//! 纯领域逻辑：不依赖任何 IO（无 tokio/SQLite/MQTT/Slint），
//! 全部行为可在 PC 上确定性单测。构建时刻由 build.rs 注入用于时钟安全判定。

pub mod clock;
pub mod command;
pub mod energy;
pub mod policy;
pub mod snapshot;

/// 固件构建时刻（毫秒），由 build.rs 通过环境变量注入；解析失败返回 0（不启用时钟拦截）。
pub fn build_epoch_ms() -> u64 {
    static CACHE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        option_env!("GW_BUILD_EPOCH_MS")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}
