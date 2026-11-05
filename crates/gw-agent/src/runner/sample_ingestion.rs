use super::*;
/// 处理一次成功采样：能量账 + 最新快照 + 不可覆盖的历史样本。
pub(super) async fn handle_sample(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    points: Vec<(String, f64)>,
) {
    let now = ctx.now_ms();
    recover_gateway_alarm(
        ctx,
        meter,
        &format!("DEVICE_OFFLINE:{}", meter.record.device_sn),
        "DEVICE_OFFLINE",
        format!("{} 采集已恢复", meter.record.device_sn),
    );
    let total = points
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(FORWARD_ACTIVE_ENERGY))
        .map(|(_, v)| *v)
        .filter(|v| v.is_finite());
    let mut quality = proto::model::QUALITY_NORMAL;
    let mut rollback_active = meter.rollback_active;
    let mut stable_readings = meter.rollback_stable_readings;
    let mut rollback_warning = None;
    let mut recovered = false;
    if let Some(current) = total {
        if rollback_active {
            if meter
                .last_total_kwh
                .map(|previous| current + energy::MONOTONIC_EPSILON >= previous)
                .unwrap_or(false)
            {
                stable_readings = stable_readings.saturating_add(1);
            } else {
                stable_readings = 0;
            }
            quality = proto::model::QUALITY_ROLLBACK;
            if stable_readings >= energy::ROLLBACK_RECOVERY_STABLE_READINGS {
                rollback_active = false;
                stable_readings = 0;
                recovered = true;
            }
        } else {
            match energy::evaluate(meter.last_total_kwh, current) {
                EnergyVerdict::Rollback { previous, current } => {
                    rollback_active = true;
                    stable_readings = 0;
                    quality = proto::model::QUALITY_ROLLBACK;
                    rollback_warning = Some((previous, current));
                }
                EnergyVerdict::Delta(delta) if delta > 0.0 => {
                    let day = energy::day_key(now, ctx.tz_offset_s);
                    if let Err(e) = ctx.store.add_energy(meter.record.id, &day, delta) {
                        tracing::warn!("电能日账写入失败: {e}");
                    }
                }
                EnergyVerdict::Delta(_) | EnergyVerdict::First => {}
            }
        }
    }
    if quality == proto::model::QUALITY_ROLLBACK {
        let should_warn = rollback_warning.is_some()
            || now.saturating_sub(meter.last_rollback_warn_ms)
                >= energy::ROLLBACK_WARN_INTERVAL_MS;
        if should_warn {
            if let Some((previous, current)) = rollback_warning {
                rollback_warning = Some((previous, current));
            } else {
                rollback_warning = Some((
                    meter.last_total_kwh.unwrap_or_default(),
                    total.unwrap_or_default(),
                ));
            }
        } else {
            rollback_warning = None;
        }
    }
    let rollback_warn_emitted = rollback_warning.is_some();
    if let Some((previous, current)) = rollback_warning {
        ctx.emitter
            .emit(
                "WARN",
                "energy",
                format!(
                    "{} 读数回退 {previous:.2} → {current:.2} kWh，样本保留为 QUALITY_ROLLBACK，不计入能耗增量",
                    meter.record.device_sn
                ),
            );
    }
    if recovered {
        ctx.emitter
            .emit(
                "INFO",
                "energy",
                format!(
                    "{} 已连续 {} 次稳定读取，已重建能耗基线；下一次正常读数开始计量",
                    meter.record.device_sn, energy::ROLLBACK_RECOVERY_STABLE_READINGS
                ),
            );
    }
    ctx.state
        .update_meter(
            meter.record.id,
            |m| {
                m.online = true;
                m.consecutive_failures = 0;
                m.last_read_ms = now;
                m.last_quality = quality;
                m.last_points = points.clone();
                if let Some(t) = total {
                    m.last_total_kwh = Some(t);
                }
                m.rollback_active = rollback_active;
                m.rollback_stable_readings = stable_readings;
                if quality == proto::model::QUALITY_ROLLBACK && rollback_warn_emitted {
                    m.last_rollback_warn_ms = now;
                }
            },
        );
    let reading = gw_store::ReadingRecord {
        meter_id: meter.record.id,
        snapshot_json: serde_json::to_string(&points).unwrap_or_default(),
        read_ms: now,
        sample_interval_s: if ctx.state.continuous_pull_enabled(meter.record.id) {
            5
        } else {
            meter.record.collect_interval_s
        },
        quality,
    };
    let result = ctx.store.save_sample(&reading, now).map(|_| ());
    if let Err(e) = result {
        tracing::warn!("读数落库失败: {e}");
    }
    evaluate_alarm_protocols(ctx, meter, &points);
}
/// 采集失败：连续 3 次判离线。
pub(super) fn handle_poll_failure(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    error: gw_collector::TransportError,
) {
    let mut went_offline = false;
    ctx.state
        .update_meter(
            meter.record.id,
            |m| {
                m.consecutive_failures += 1;
                if m.consecutive_failures == 3 {
                    m.online = false;
                    went_offline = true;
                }
            },
        );
    if went_offline {
        raise_gateway_alarm(
            ctx,
            meter,
            &format!("DEVICE_OFFLINE:{}", meter.record.device_sn),
            "DEVICE_OFFLINE",
            "WARN",
            format!("{} 连续 3 次采集失败", meter.record.device_sn),
        );
        ctx.emitter
            .emit(
                "WARN",
                "网关告警",
                format!(
                    "{} 连续 3 次采集失败，判定离线（缺采由云侧断采告警接管）",
                    meter.record.device_sn
                ),
            );
    }
    tracing::debug!("采集失败 {}: {error}", meter.record.device_sn);
}
