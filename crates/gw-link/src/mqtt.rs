//! MQTT 云链路实现（rumqttc，QoS1）。
//!
//! 自持连接生命周期：内部 eventloop 任务负责重连与状态上报，
//! 指令下行经 mpsc 通道交给 agent；publish 只是入 AsyncClient 内部队列（有界）。

use std::time::Duration;

use async_trait::async_trait;
use gw_core::snapshot::LinkHealth;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Outgoing, Packet, QoS};
use tokio::sync::{mpsc, watch};

use crate::port::{CloudLink, LinkError, LinkOptions, LinkResult, RawCommand};

/// 带指令订阅的 MQTT 链路。
pub struct MqttLink {
    client: AsyncClient,
    health_rx: watch::Receiver<LinkHealth>,
}

impl MqttLink {
    /// 建立连接并启动内部 eventloop 任务（含自动重连与状态推送）。
    /// 返回链路与指令接收端；任务句柄由调用方持有以便 join。
    pub fn spawn(
        opts: LinkOptions,
        command_down_topic: String,
        commands: mpsc::Sender<RawCommand>,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let alarm_ack_topic = format!("gateway/{}/alarm/ack", opts.gateway_id);
        let mut mqtt = MqttOptions::new(&opts.client_id, &opts.host, opts.port);
        mqtt.set_keep_alive(Duration::from_secs(30));
        mqtt.set_max_packet_size(256 * 1024, 256 * 1024);
        if !opts.username.is_empty() {
            mqtt.set_credentials(&opts.username, &opts.password);
        }

        let (client, mut eventloop) = AsyncClient::new(mqtt, 64);
        let (health_tx, health_rx) = watch::channel(LinkHealth::Connecting);

        // 订阅指令下行主题（QoS1）
        let subscribe_client = client.clone();
        let topics = vec![command_down_topic.clone(), alarm_ack_topic.clone()];
        tokio::spawn(async move {
            for topic in topics {
                if let Err(e) = subscribe_client.subscribe(topic, QoS::AtLeastOnce).await {
                    tracing::warn!("订阅下行主题失败（将在重连后重试）: {e}");
                }
            }
        });

        let loop_client = client.clone();
        let handle = tokio::spawn(async move {
            let client = loop_client;
            let _ = health_tx.send_replace(LinkHealth::Connecting);
            let mut backoff = Duration::from_secs(1);
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let topic = publish.topic.clone();
                        let payload = publish.payload.to_vec();
                        if commands.send(RawCommand { topic, payload }).await.is_err() {
                            // agent 已退出
                            break;
                        }
                    }
                    Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                        backoff = Duration::from_secs(1);
                        let _ = health_tx.send_replace(LinkHealth::Connected);
                        // 每次重连后补订阅（broker 不保留会话时必要）
                        for topic in [command_down_topic.clone(), alarm_ack_topic.clone()] {
                            let _ = client.subscribe(topic, QoS::AtLeastOnce).await;
                        }
                    }
                    Ok(Event::Outgoing(Outgoing::PingReq)) | Ok(_) => {
                        // poll 正常返回即连接存活
                        if *health_tx.borrow() != LinkHealth::Connected {
                            let _ = health_tx.send_replace(LinkHealth::Connected);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("MQTT 链路异常，将退避重连: {e}");
                        let _ = health_tx.send_replace(LinkHealth::Down);
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                    }
                }
            }
        });

        (Self { client, health_rx }, handle)
    }
}

#[async_trait]
impl CloudLink for MqttLink {
    async fn publish(&self, topic: &str, payload: &[u8]) -> LinkResult<()> {
        if *self.health_rx.borrow() != LinkHealth::Connected {
            return Err(LinkError::NotConnected);
        }
        self.client
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| LinkError::Mqtt(format!("publish 入队失败: {e}")))
    }

    fn health(&self) -> LinkHealth {
        *self.health_rx.borrow()
    }
}
