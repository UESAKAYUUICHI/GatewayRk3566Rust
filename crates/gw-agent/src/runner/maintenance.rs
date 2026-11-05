use super::*;
/// 快照任务：1Hz 聚合状态给观察者。
pub(super) async fn snapshotter_task(
    ctx: TaskCtx,
    snapshot_tx: watch::Sender<Snapshot>,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return, _ = ticker.tick() => {}
        }
        let now = ctx.now_ms();
        let day = energy::day_key(now, ctx.tz_offset_s);
        let (online, total) = ctx.state.online_counts();
        let meters: Vec<MeterSnapshot> = ctx
            .state
            .meters_snapshot()
            .into_iter()
            .map(|m| MeterSnapshot {
                id: m.record.id,
                device_sn: m.record.device_sn.clone(),
                online: m.online,
                addr: m.record.modbus_addr,
                interval_s: m.record.collect_interval_s,
                continuous_pull: ctx.state.continuous_pull_enabled(m.record.id),
                enabled: m.record.enabled,
                profile: m.record.profile.clone(),
                last_read_ms: m.last_read_ms,
                quality: m.last_quality,
                points: m.last_points.clone(),
                today_kwh: ctx.store.energy_of(m.record.id, &day).unwrap_or(0.0),
            })
            .collect();
        let snapshot = Snapshot {
            cloud: ctx.link.health(),
            clock_trusted: ctx.clock_trusted(),
            outbox_pending: ctx.store.outbox_pending_count().unwrap_or(0),
            outbox_sent_total: ctx.store.outbox_sent_count().unwrap_or(0),
            meters_online: online,
            meters_total: total,
            today_kwh_total: ctx.store.energy_sum(&day).unwrap_or(0.0),
            last_publish_ms: ctx.state.last_publish_ms(),
            last_heartbeat_ms: ctx.state.last_heartbeat_ms(),
            started_ms: ctx.state.started_ms,
            version: env!("CARGO_PKG_VERSION").to_string(),
            meters,
        };
        let _ = snapshot_tx.send(snapshot);
    }
}
/// 维护任务：sent 行 7 天滚动清理 + 事件裁剪。
pub(super) async fn maintainer_task(
    store: Arc<Store>,
    clock: Arc<dyn ClockSource>,
    tz_offset_s: i32,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(3600));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return, _ = ticker.tick() => {}
        }
        if let Some((week_start, week_end)) = weekly_archive_window(
            clock.now_ms(),
            tz_offset_s,
        ) {
            let last_archived = store
                .get_config("weekly_archive_last_start_ms")
                .ok()
                .flatten()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if last_archived < week_start {
                match store.archive_weekly_samples(week_start, week_end, clock.now_ms())
                {
                    Ok((summary_count, deleted)) => {
                        let _ = store
                            .set_config(
                                "weekly_archive_last_start_ms",
                                &week_start.to_string(),
                            );
                        tracing::info!(
                            "周汇总完成: {summary_count} 台设备，删除 {deleted} 条上一周明细"
                        );
                    }
                    Err(e) => tracing::warn!("周汇总失败: {e}"),
                }
            }
        }
        let cutoff = clock.now_ms().saturating_sub(7 * 24 * 3600 * 1000);
        if let Err(e) = store.cleanup_samples_before(cutoff) {
            tracing::warn!("历史样本清理失败: {e}");
        }
        if let Err(e) = store.cleanup_local_samples_before(cutoff) {
            tracing::warn!("本地设备历史样本清理失败: {e}");
        }
        match store.cleanup_sent_before(cutoff) {
            Ok(n) if n > 0 => tracing::info!("清理 {n} 条已发布 outbox 行"),
            Ok(_) => {}
            Err(e) => tracing::warn!("outbox 清理失败: {e}"),
        }
        if let Err(e) = store.trim_events(5000) {
            tracing::warn!("事件裁剪失败: {e}");
        }
    }
}
pub(super) fn weekly_archive_window(
    now_ms: u64,
    tz_offset_s: i32,
) -> Option<(u64, u64)> {
    const DAY_MS: i64 = 86_400_000;
    const WEEK_MS: i64 = 7 * DAY_MS;
    let local_ms = now_ms as i64 + tz_offset_s as i64 * 1000;
    let local_day = local_ms.div_euclid(DAY_MS);
    let ms_of_day = local_ms.rem_euclid(DAY_MS);
    let weekday_monday0 = (local_day + 3).rem_euclid(7);
    if weekday_monday0 != 0 || ms_of_day < 2 * 3_600_000 {
        return None;
    }
    let current_week_start_local_ms = (local_day * DAY_MS) - weekday_monday0 * DAY_MS;
    let previous_week_start_local_ms = current_week_start_local_ms - WEEK_MS;
    let offset_ms = tz_offset_s as i64 * 1000;
    Some((
        (previous_week_start_local_ms - offset_ms) as u64,
        (current_week_start_local_ms - offset_ms) as u64,
    ))
}
