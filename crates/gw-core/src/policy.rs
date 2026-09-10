//! 配置与出网前校验策略，规则与云侧约束镜像：
//! - 采集间隔下限 1s（云侧限流 120 报/分/设备 = 每 0.5s 一报，留安全余量）
//! - collectTime 校验与 `DataIngestService.validateQuality` 一致（未来 10min / 90 天）

/// 采集周期允许范围（秒）。
pub const MIN_COLLECT_INTERVAL_S: u64 = 1;
pub const MAX_COLLECT_INTERVAL_S: u64 = 86_400;

/// 云侧判离线阈值（秒）。心跳周期建议不超过其 1/3。
pub const CLOUD_OFFLINE_THRESHOLD_S: u64 = 90;

/// 校验采集周期。
pub fn validate_collect_interval(seconds: u64) -> Result<u64, String> {
    if seconds < MIN_COLLECT_INTERVAL_S {
        return Err(format!(
            "采集周期不得小于 {MIN_COLLECT_INTERVAL_S}s（云侧限流 120 报/分/设备）"
        ));
    }
    if seconds > MAX_COLLECT_INTERVAL_S {
        return Err(format!("采集周期不得大于 {}s", MAX_COLLECT_INTERVAL_S));
    }
    Ok(seconds)
}

/// 校验心跳周期：必须明显小于云侧离线阈值，避免抖动导致的在线/离线抖动。
pub fn validate_heartbeat_interval(seconds: u64) -> Result<u64, String> {
    if seconds == 0 || seconds > CLOUD_OFFLINE_THRESHOLD_S / 3 {
        return Err(format!(
            "心跳周期须在 1~{}s 之间（云侧 {}s 无心跳判离线）",
            CLOUD_OFFLINE_THRESHOLD_S / 3,
            CLOUD_OFFLINE_THRESHOLD_S
        ));
    }
    Ok(seconds)
}

/// 设备编号规则：与平台 `dev_device.device_sn` 对齐的非空字符串。
pub fn validate_device_sn(sn: &str) -> Result<(), String> {
    let trimmed = sn.trim();
    if trimmed.is_empty() {
        return Err("设备编号不能为空".to_string());
    }
    if trimmed.len() > 64 {
        return Err("设备编号长度不能超过 64".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err("设备编号仅允许字母、数字、-、_、.".to_string());
    }
    Ok(())
}

/// 出网前镜像云侧 collectTime 合法性校验（DataIngestService.validateQuality）。
/// future_slack_ms 默认 10 分钟，max_age_ms 默认 90 天。
pub fn collect_time_publishable(
    collect_ms: u64,
    now_ms: u64,
    future_slack_ms: u64,
    max_age_ms: u64,
) -> bool {
    if collect_ms > now_ms.saturating_add(future_slack_ms) {
        return false;
    }
    if collect_ms + max_age_ms < now_ms {
        return false;
    }
    true
}

/// 默认参数版本的出网校验（10 分钟 / 90 天，与云侧常量一致）。
pub fn collect_time_publishable_default(collect_ms: u64, now_ms: u64) -> bool {
    collect_time_publishable(collect_ms, now_ms, 10 * 60 * 1000, 90 * 24 * 3600 * 1000)
}
