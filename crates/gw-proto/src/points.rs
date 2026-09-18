//! 标准测点键与单位。
//! 键名必须与平台发布的产品测点编码保持一致，
//! 云侧 JsonPointParser 依据映射表把原始键转为 VOLTAGE_A 等标准测点编码。

pub const VOLTAGE_A: &str = "voltage_a";
pub const VOLTAGE_B: &str = "voltage_b";
pub const VOLTAGE_C: &str = "voltage_c";
pub const CURRENT_A: &str = "current_a";
pub const CURRENT_B: &str = "current_b";
pub const CURRENT_C: &str = "current_c";
pub const POWER_FACTOR_TOTAL: &str = "power_factor_total";
pub const FREQUENCY: &str = "frequency";
pub const ACTIVE_POWER_TOTAL: &str = "active_power_total";
pub const REACTIVE_POWER_TOTAL: &str = "reactive_power_total";
pub const APPARENT_POWER_TOTAL: &str = "apparent_power_total";
pub const FORWARD_ACTIVE_ENERGY: &str = "forward_active_energy";

/// 与平台测点映射一致的标准测点键集合。
pub fn standard_keys() -> [&'static str; 12] {
    [
        VOLTAGE_A,
        VOLTAGE_B,
        VOLTAGE_C,
        CURRENT_A,
        CURRENT_B,
        CURRENT_C,
        POWER_FACTOR_TOTAL,
        FREQUENCY,
        ACTIVE_POWER_TOTAL,
        REACTIVE_POWER_TOTAL,
        APPARENT_POWER_TOTAL,
        FORWARD_ACTIVE_ENERGY,
    ]
}

/// 测点显示单位（UI 用）。
pub fn unit_of(key: &str) -> &'static str {
    match key {
        VOLTAGE_A | VOLTAGE_B | VOLTAGE_C => "V",
        CURRENT_A | CURRENT_B | CURRENT_C => "A",
        POWER_FACTOR_TOTAL => "",
        FREQUENCY => "Hz",
        ACTIVE_POWER_TOTAL => "kW",
        REACTIVE_POWER_TOTAL => "kvar",
        APPARENT_POWER_TOTAL => "kVA",
        FORWARD_ACTIVE_ENERGY => "kWh",
        _ => "",
    }
}
