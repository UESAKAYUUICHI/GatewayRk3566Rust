//! 时钟安全策略。RK3566 无 RTC 电池时开机时钟可能回退到很久以前，
//! 而云侧 `DataIngestService.validateQuality` 会拒收「未来 >10min」或「超 90 天」
//! 的 collectTime —— 因此时钟不可信期间一律不出网，数据照常入 outbox。

/// 时钟可信度判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockTrust {
    /// 时钟可信，可以出网
    Trusted,
    /// 系统时间早于固件构建时刻，几乎必然是未完成 NTP 同步的假时钟
    BehindBuildTime,
}

/// 基于构建时刻的时钟可信度评估器。
#[derive(Debug, Clone, Copy)]
pub struct ClockAssessor {
    build_epoch_ms: u64,
}

impl ClockAssessor {
    pub fn new(build_epoch_ms: u64) -> Self {
        Self { build_epoch_ms }
    }

    /// 当前时间早于构建时刻（含 10 分钟宽限，容忍构建机与设备时钟的小偏差）即判不可信。
    pub fn assess(&self, now_ms: u64) -> ClockTrust {
        if self.build_epoch_ms == 0 {
            return ClockTrust::Trusted;
        }
        if now_ms + 10 * 60 * 1000 < self.build_epoch_ms {
            ClockTrust::BehindBuildTime
        } else {
            ClockTrust::Trusted
        }
    }
}

impl Default for ClockAssessor {
    fn default() -> Self {
        Self::new(crate::build_epoch_ms())
    }
}
