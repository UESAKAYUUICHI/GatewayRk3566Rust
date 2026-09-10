//! 电能累积量规则：读数单调性校验与日增量归属。
//!
//! 云侧以 `FORWARD_ACTIVE_ENERGY`（正向有功总电能）差值计算用量，
//! 因此网关必须保证上报读数单调；表清零/换表会触发回退，该样本要拦下并留痕。

/// 读数回退比较容差（kWh），吸收浮点噪声。
const MONOTONIC_EPSILON: f64 = 1e-6;

/// 对一次新读数的判定结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnergyVerdict {
    /// 首次见到该表，作为基线，不产生增量
    First,
    /// 正常增量（可为 0，表示无用电）
    Delta(f64),
    /// 读数回退：表清零/换表，样本不应上报，需重新建立基线
    Rollback { previous: f64, current: f64 },
}

/// 判定新读数是否保持单调。
pub fn evaluate(previous: Option<f64>, current: f64) -> EnergyVerdict {
    match previous {
        None => EnergyVerdict::First,
        Some(prev) if current + MONOTONIC_EPSILON >= prev => EnergyVerdict::Delta(current - prev),
        Some(prev) => EnergyVerdict::Rollback {
            previous: prev,
            current,
        },
    }
}

/// 由 Unix 毫秒 + 时区偏移秒数得到本地日期键 `YYYY-MM-DD`。
/// 使用 Howard Hinnant 的 civil_from_days 算法，无 chrono 依赖、结果确定性。
pub fn day_key(ts_ms: u64, tz_offset_s: i32) -> String {
    let (y, m, d) = civil_from_days(days_from_unix_ms(ts_ms, tz_offset_s));
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_from_unix_ms(ts_ms: u64, tz_offset_s: i32) -> i64 {
    let shifted = ts_ms as i64 + tz_offset_s as i64 * 1000;
    shifted.div_euclid(86_400_000)
}

/// 天数 → (年, 月, 日)，civil calendar。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
