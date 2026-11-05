use super::*;
/// 单表采集任务。周期每轮从状态重读（SET_INTERVAL 即时生效）；
/// 表删除/停用由 sync_pollers 取消令牌，本任务自然退出。
pub(super) async fn poller_task(
    ctx: TaskCtx,
    meter_id: i64,
    token: Arc<CancellationToken>,
    shutdown: CancellationToken,
) {
    loop {
        let configured_interval_s = ctx
            .state
            .meter_by_id(meter_id)
            .map(|m| m.record.collect_interval_s)
            .unwrap_or(0);
        let interval_s = if ctx.state.continuous_pull_enabled(meter_id) {
            5
        } else {
            configured_interval_s
        };
        if interval_s == 0 {
            return;
        }
        tokio::select! {
            _ = shutdown.cancelled() => return, _ = token.cancelled() => return, _ =
            tokio::time::sleep(Duration::from_secs(interval_s.max(1))) => {}
        }
        let Some(meter) = ctx.state.meter_by_id(meter_id) else {
            return;
        };
        if !ctx.store.channel_enabled(&meter.record.channel_id).unwrap_or(false) {
            let event_id = format!("CHANNEL_DISABLED:{}", meter.record.channel_id);
            raise_gateway_alarm(
                &ctx,
                &meter,
                &event_id,
                "CHANNEL_DISABLED",
                "MAJOR",
                format!("RS485 通道 {} 未启用或不存在", meter.record.channel_id),
            );
            continue;
        }
        recover_gateway_alarm(
            &ctx,
            &meter,
            &format!("CHANNEL_DISABLED:{}", meter.record.channel_id),
            "CHANNEL_DISABLED",
            format!("RS485 通道 {} 已恢复", meter.record.channel_id),
        );
        let rows = ctx.store.register_map(&meter.record.profile).unwrap_or_default();
        if rows.is_empty() {
            ctx.emitter
                .emit(
                    "WARN",
                    "poller",
                    format!(
                        "档案 {} 为空，跳过采集 {}", meter.record.profile, meter
                        .record.device_sn
                    ),
                );
            continue;
        }
        let specs = specs_of(&rows);
        let Some(transport) = ensure_channel_transport(&ctx, &meter).await else {
            handle_channel_unavailable(&ctx, &meter);
            continue;
        };
        recover_gateway_alarm(
            &ctx,
            &meter,
            &format!("CHANNEL_UNAVAILABLE:{}", meter.record.channel_id),
            "CHANNEL_UNAVAILABLE",
            format!("RS485 通道 {} 传输已恢复", meter.record.channel_id),
        );
        match poll_once(&transport, &meter.record, &specs).await {
            Ok(points) => handle_sample(&ctx, &meter, points).await,
            Err(e) => handle_poll_failure(&ctx, &meter, e),
        }
    }
}
pub(super) fn handle_channel_unavailable(ctx: &TaskCtx, meter: &MeterRuntime) {
    raise_gateway_alarm(
        ctx,
        meter,
        &format!("CHANNEL_UNAVAILABLE:{}", meter.record.channel_id),
        "CHANNEL_UNAVAILABLE",
        "MAJOR",
        format!("RS485 通道 {} 未建立串口传输", meter.record.channel_id),
    );
}
