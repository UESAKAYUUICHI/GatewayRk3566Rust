//! 云链路端口：发布（QoS1）+ 指令订阅 + 健康状态。

use async_trait::async_trait;
use gw_core::snapshot::LinkHealth;

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
    #[error("MQTT 操作失败: {0}")]
    Mqtt(String),
    #[error("链路未连接")]
    NotConnected,
}

pub type LinkResult<T> = Result<T, LinkError>;

/// 链路连接参数。
#[derive(Debug, Clone)]
pub struct LinkOptions {
    pub host: String,
    pub port: u16,
    /// 空则匿名连接。
    pub username: String,
    pub password: String,
    pub client_id: String,
    /// 云端网关 ID（topic 组成部分）
    pub gateway_id: String,
}

/// 链路收到的下行指令原文。
#[derive(Debug, Clone)]
pub struct RawCommand {
    pub topic: String,
    pub payload: Vec<u8>,
}

/// 云链路端口。实现负责自身的连接生命周期与重连；
/// 发布在未连接时应返回 `LinkError::NotConnected` 而非阻塞。
#[async_trait]
pub trait CloudLink: Send + Sync {
    /// 以 QoS1 发布。返回仅代表报文已入队/送达 broker（实现决定语义并负责重试）。
    async fn publish(&self, topic: &str, payload: &[u8]) -> LinkResult<()>;

    /// 当前健康状态。
    fn health(&self) -> LinkHealth;
}
