use super::*;
/// 发布器：唯一出网出口，按 id 序发布；deferred 行在时钟可信后定稿。
pub(super) async fn publisher_task(ctx: TaskCtx, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return, _ = ticker.tick() => {}
        }
        if !ctx.clock_trusted() {
            continue;
        }
        if !ctx.link.health().is_publishable() {
            continue;
        }
        if let Ok(deferred) = ctx.store.deferred_outbox(64) {
            for row in deferred {
                let ts = ctx.state.next_message_ts(ctx.now_ms());
                match promote_deferred(&row.payload, ts, &ctx.state.gateway_id()) {
                    Ok((message_id, payload)) => {
                        if let Err(e) = ctx
                            .store
                            .finalize_outbox(row.id, &message_id, &payload, ts)
                        {
                            tracing::warn!("deferred 定稿失败 id={}: {e}", row.id);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("deferred 报文解析失败 id={}: {e}", row.id);
                        if let Err(error) = ctx.store.quarantine_outbox(row.id, &e.to_string()) {
                            tracing::error!("无法隔离损坏报文 id={}: {error}", row.id);
                        }
                    }
                }
            }
        }
        let Ok(pending) = ctx.store.pending_outbox(16) else {
            continue;
        };
        for row in pending {
            match ctx.link.publish(&row.topic, &row.payload).await {
                Ok(()) => {
                    if let Err(error) = ctx.store.mark_sent(row.id) {
                        tracing::error!("发布已确认，但本地确认失败 id={}: {error}", row.id);
                        break;
                    }
                    if row.topic.ends_with("/alarm/up") {
                        if let Ok(alarm) = serde_json::from_slice::<
                            proto::GatewayAlarmPayload,
                        >(&row.payload) {
                            let _ = ctx
                                .store
                                .mark_alarm_cloud_ack(
                                    &alarm.event_id,
                                    "MQTT_ACKED",
                                    ctx.now_ms(),
                                );
                        }
                    }
                    ctx.state.mark_published(ctx.now_ms());
                    tracing::debug!("已发布 {} ({})", row.message_id, row.topic);
                }
                Err(e) => {
                    if let Err(error) = ctx.store.mark_attempt_failed(row.id, &e.to_string()) {
                        tracing::error!("记录发布失败 id={}: {error}", row.id);
                    }
                    tracing::debug!("发布失败 {} : {e}", row.message_id);
                    break;
                }
            }
        }
    }
}
/// Preserve measurement times; untrusted clock data is retained but cannot enter accounting or alarm evaluation.
pub(super) fn promote_deferred(
    payload: &[u8],
    ts: u64,
    gateway_id: &str,
) -> Result<(String, Vec<u8>), serde_json::Error> {
    let mut upload: proto::GatewayUploadPayload = serde_json::from_slice(payload)?;
    upload.timestamp = ts;
    for meter in &mut upload.meters {
        meter.quality |= proto::model::QUALITY_CLOCK_UNTRUSTED;
    }
    upload.message_id = proto::ids::data_message_id(gateway_id, ts);
    let bytes = serde_json::to_vec(&upload)?;
    Ok((upload.message_id, bytes))
}
/// 心跳任务：仅在当时钟可信时发送。
pub(super) async fn heartbeat_task(ctx: TaskCtx, shutdown: CancellationToken) {
    loop {
        let heartbeat_s = ctx.state.heartbeat_s().max(1);
        tokio::select! {
            _ = shutdown.cancelled() => return, _ =
            tokio::time::sleep(Duration::from_secs(heartbeat_s)) => {}
        }
        if !ctx.clock_trusted() {
            continue;
        }
        let now = ctx.state.next_message_ts(ctx.now_ms());
        ctx.state.mark_heartbeat(now);
        if !ctx.link.health().is_publishable() {
            continue;
        }
        let gateway_id = ctx.state.gateway_id();
        let devices = ctx
            .state
            .meters_snapshot()
            .into_iter()
            .map(|meter| proto::DeviceHeartbeat {
                device_sn: meter.record.device_sn,
                online: meter.online,
                last_read_time: meter.last_read_ms,
                quality: meter.last_quality,
            })
            .collect();
        let pending = ctx.store.outbox_pending_count().unwrap_or(0);
        let payload = proto::HeartbeatPayload::new_online(
            proto::ids::heartbeat_message_id(&gateway_id, now),
            ctx.state.gateway_sn(),
            now,
            devices,
            pending,
        );
        let Ok(bytes) = serde_json::to_vec(&payload) else {
            continue;
        };
        let topic = topics::heartbeat_topic(&gateway_id);
        if let Err(e) = ctx.link.publish(&topic, &bytes).await {
            tracing::debug!("心跳发布失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_clock_recovery_preserves_observed_time_and_marks_quality() {
        let sample = proto::MeterSample {
            device_sn:"meter".into(),modbus_addr:1,channel_id:"bus".into(),
            profile_key:"profile".into(),model_version:"1".into(),config_revision:None,
            collect_time:123,sample_interval_seconds:5,quality:0,points:Default::default(),
        };
        let payload = proto::GatewayUploadPayload::new_data(
            "deferred".into(),"gateway".into(),123,10,vec![sample]);
        let (_, bytes) = promote_deferred(&serde_json::to_vec(&payload).unwrap(),10000,"3").unwrap();
        let promoted: proto::GatewayUploadPayload = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(promoted.meters[0].collect_time,123);
        assert_eq!(promoted.timestamp,10000);
        assert_eq!(promoted.meters[0].quality,proto::model::QUALITY_CLOCK_UNTRUSTED);
    }
}
