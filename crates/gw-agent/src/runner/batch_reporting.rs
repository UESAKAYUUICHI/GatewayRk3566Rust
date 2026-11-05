use super::*;
/// 周期性把所有尚未归批的中间样本装入一条 DATA_UPLOAD。SQLite 事务保证样本与 outbox 不会脱节。
pub(super) async fn report_batch_task(ctx: TaskCtx, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return, _ =
            tokio::time::sleep(next_report_boundary(ctx.now_ms(), ctx.report_interval_s))
            => flush_sample_batch(& ctx). await,
        }
    }
}
pub(super) fn next_report_boundary(now_ms: u64, interval_s: u64) -> Duration {
    let interval_ms = interval_s.max(1).saturating_mul(1_000);
    let next_ms = (now_ms / interval_ms + 1).saturating_mul(interval_ms);
    Duration::from_millis(next_ms.saturating_sub(now_ms))
}
pub(super) async fn flush_sample_batch(ctx: &TaskCtx) {
    let Ok(rows) = ctx.store.unbatched_samples(2_000) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    let mut samples = Vec::with_capacity(rows.len());
    let mut sample_ids = Vec::with_capacity(rows.len());
    for row in rows {
        let Ok(points) = serde_json::from_str::<Vec<(String, f64)>>(&row.points_json)
        else {
            ctx.emitter
                .emit(
                    "ERROR",
                    "batch",
                    format!("历史样本 {} 无法解析，暂不归批", row.id),
                );
            continue;
        };
        samples
            .push(proto::MeterSample {
                device_sn: row.device_sn,
                modbus_addr: row.modbus_addr,
                channel_id: row.channel_id,
                profile_key: row.profile,
                model_version: row.model_version,
                config_revision: row.config_revision,
                collect_time: row.read_ms,
                sample_interval_seconds: row.sample_interval_s,
                quality: row.quality,
                points: points.into_iter().collect(),
            });
        sample_ids.push(row.id);
    }
    if samples.is_empty() {
        return;
    }
    let trusted = ctx.clock_trusted();
    let ts = ctx.state.next_message_ts(ctx.now_ms());
    let status = if trusted { "pending" } else { "deferred" };
    let gateway_id = ctx.state.gateway_id();
    let message_id = if trusted {
        proto::ids::data_message_id(&gateway_id, ts)
    } else {
        format!("DFR-BATCH-{}", ctx.state.next_defer_seq())
    };
    let payload = proto::GatewayUploadPayload::new_data(
        message_id.clone(),
        ctx.state.gateway_sn(),
        ts,
        ctx.report_interval_s,
        samples,
    );
    let Ok(bytes) = serde_json::to_vec(&payload) else {
        tracing::error!("报文序列化失败: {message_id}");
        return;
    };
    let item = gw_store::NewOutbox {
        message_id,
        topic: topics::data_upload_topic(&gateway_id),
        payload: bytes,
        created_ms: ctx.now_ms(),
    };
    match ctx.store.enqueue_sample_batch(&item, status, &sample_ids) {
        Ok(true) => {
            ctx.notify_publish.notify_one();
            ctx.emitter
                .emit(
                    "INFO",
                    "batch",
                    format!("已归批 {} 个采样点", sample_ids.len()),
                );
        }
        Ok(false) => {}
        Err(e) => tracing::warn!("outbox 入队失败: {e}"),
    }
}
