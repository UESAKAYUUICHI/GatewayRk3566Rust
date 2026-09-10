//! 寄存器档案：把 Modbus 寄存器布局表达为数据（profiles/*.toml），
//! 与平台侧 `dev_point_mapping` 的配置化思想镜像 —— 新表型零代码接入。

use std::collections::BTreeMap;

use serde::Deserialize;

/// 解码后的数值类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    /// 单寄存器无符号
    U16,
    /// 单寄存器有符号
    I16,
    /// 双寄存器无符号（高字在前）
    U32,
    /// 双寄存器有符号（高字在前）
    I32,
    /// IEEE754 32-bit 浮点数
    F32,
}

impl DataType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataType::U16 => "u16",
            DataType::I16 => "i16",
            DataType::U32 => "u32",
            DataType::I32 => "i32",
            DataType::F32 => "f32",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "u16" | "U16" => Some(DataType::U16),
            "i16" | "I16" => Some(DataType::I16),
            "u32" | "U32" => Some(DataType::U32),
            "i32" | "I32" => Some(DataType::I32),
            "f32" | "F32" | "float32" | "FLOAT32" => Some(DataType::F32),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(data_type: DataType, byte_order: &str) -> RegisterSpec {
        RegisterSpec {
            point: "p".into(),
            func: 3,
            address: 0,
            quantity: match data_type {
                DataType::U16 | DataType::I16 => 1,
                DataType::U32 | DataType::I32 | DataType::F32 => 2,
            },
            data_type,
            byte_order: byte_order.into(),
            scale: 1.0,
            offset: 0.0,
        }
    }

    #[test]
    fn decodes_word_and_byte_orders() {
        assert_eq!(
            MeterProfile::decode_one(&spec(DataType::U32, "ABCD"), &[0x0001, 0x0002]),
            65_538.0
        );
        assert_eq!(
            MeterProfile::decode_one(&spec(DataType::U32, "CDAB"), &[0x0001, 0x0002]),
            131_073.0
        );
        assert_eq!(
            MeterProfile::decode_one(&spec(DataType::U32, "BADC"), &[0x0100, 0x0200]),
            65_538.0
        );
    }

    #[test]
    fn decodes_float32_with_low_word_first() {
        let value = MeterProfile::decode_one(&spec(DataType::F32, "CDAB"), &[0x0000, 0x42f6]);
        assert!((value - 123.0).abs() < 0.0001);
    }
}

/// 单个测点的寄存器定义。
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterSpec {
    /// 测点键（与上报报文 points 键一致，如 voltage_a）
    pub point: String,
    /// 功能码，当前仅支持 3（保持寄存器）
    #[serde(default = "default_func")]
    pub func: u8,
    pub address: u16,
    #[serde(default = "default_quantity")]
    pub quantity: u16,
    #[serde(default = "default_data_type")]
    pub data_type: DataType,
    #[serde(default = "default_byte_order")]
    pub byte_order: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
}

fn default_func() -> u8 {
    3
}
fn default_quantity() -> u16 {
    1
}
fn default_data_type() -> DataType {
    DataType::U16
}
fn default_scale() -> f64 {
    1.0
}
fn default_byte_order() -> String {
    "ABCD".to_string()
}

/// 一个表型的完整档案。
#[derive(Debug, Clone, Deserialize)]
pub struct MeterProfile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 平台设备类型编码。仅作契约/运维对照，不限制自定义设备使用该档案。
    #[serde(default)]
    pub device_type_codes: Vec<String>,
    pub register: Vec<RegisterSpec>,
}

#[derive(thiserror::Error, Debug)]
pub enum ProfileError {
    #[error("档案 TOML 解析失败: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("档案非法: {0}")]
    Invalid(String),
}

impl MeterProfile {
    /// 从 TOML 文本解析并校验。
    pub fn from_toml_str(text: &str) -> Result<Self, ProfileError> {
        let profile: MeterProfile = toml::from_str(text)?;
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.name.trim().is_empty() {
            return Err(ProfileError::Invalid("name 不能为空".into()));
        }
        if self.register.is_empty() {
            return Err(ProfileError::Invalid("至少需要一条寄存器定义".into()));
        }
        let mut points = std::collections::BTreeSet::new();
        for reg in &self.register {
            if reg.point.trim().is_empty() {
                return Err(ProfileError::Invalid("point 不能为空".into()));
            }
            if !points.insert(reg.point.trim()) {
                return Err(ProfileError::Invalid(format!("测点 {} 重复", reg.point)));
            }
            if !matches!(reg.func, 3 | 4) {
                return Err(ProfileError::Invalid(format!(
                    "测点 {} 仅支持功能码 3/4",
                    reg.point
                )));
            }
            let need = match reg.data_type {
                DataType::U16 | DataType::I16 => 1,
                DataType::U32 | DataType::I32 | DataType::F32 => 2,
            };
            if reg.quantity != need {
                return Err(ProfileError::Invalid(format!(
                    "测点 {} 的 quantity 应为 {need}",
                    reg.point
                )));
            }
            if !valid_byte_order(reg.quantity, &reg.byte_order) {
                return Err(ProfileError::Invalid(format!(
                    "测点 {} 的 byte_order 不合法",
                    reg.point
                )));
            }
        }
        Ok(())
    }

    /// 解码一个测点。words 长度须 ≥ quantity。
    pub fn decode_one(spec: &RegisterSpec, words: &[u16]) -> f64 {
        let ordered = ordered_bytes(words, spec.quantity, &spec.byte_order);
        let raw: f64 = match spec.data_type {
            DataType::U16 => u16::from_be_bytes([ordered[0], ordered[1]]) as f64,
            DataType::I16 => i16::from_be_bytes([ordered[0], ordered[1]]) as f64,
            DataType::U32 => {
                u32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f64
            }
            DataType::I32 => {
                i32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f64
            }
            DataType::F32 => f32::from_bits(u32::from_be_bytes([
                ordered[0], ordered[1], ordered[2], ordered[3],
            ])) as f64,
        };
        raw * spec.scale + spec.offset
    }

    /// 按档案顺序解码全部测点。
    pub fn decode_all(
        &self,
        words_of: impl Fn(&RegisterSpec) -> Vec<u16>,
    ) -> BTreeMap<String, f64> {
        let mut points = BTreeMap::new();
        for spec in &self.register {
            let words = words_of(spec);
            if words.len() >= spec.quantity as usize {
                points.insert(spec.point.clone(), Self::decode_one(spec, &words));
            }
        }
        points
    }
}

fn valid_byte_order(quantity: u16, order: &str) -> bool {
    let normalized = order.trim().to_ascii_uppercase();
    match quantity {
        1 => matches!(normalized.as_str(), "AB" | "BA" | "ABCD" | ""),
        2 => matches!(normalized.as_str(), "ABCD" | "BADC" | "CDAB" | "DCBA" | ""),
        _ => false,
    }
}

fn ordered_bytes(words: &[u16], quantity: u16, order: &str) -> Vec<u8> {
    let mut source = Vec::with_capacity(quantity as usize * 2);
    for word in words.iter().take(quantity as usize) {
        source.extend_from_slice(&word.to_be_bytes());
    }
    let normalized = order.trim().to_ascii_uppercase();
    match (quantity, normalized.as_str()) {
        (1, "BA") => vec![source[1], source[0]],
        (2, "BADC") => vec![source[1], source[0], source[3], source[2]],
        (2, "CDAB") => vec![source[2], source[3], source[0], source[1]],
        (2, "DCBA") => vec![source[3], source[2], source[1], source[0]],
        _ => source,
    }
}
