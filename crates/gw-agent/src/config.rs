//! 引导配置（TOML）。三层合并中的第二层：内置默认 < TOML 引导 < 数据库运行时。

use serde::Deserialize;

/// 云端对接参数。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CloudSection {
    /// 平台 dev_gateway.id（topic 中的网关 ID）
    pub gateway_id: String,
    /// 平台 dev_gateway.gateway_sn
    pub gateway_sn: String,
    pub mqtt_host: String,
    pub mqtt_port: u16,
    /// 建议等于 gatewaySn；为空则匿名连接（仅限联调）
    pub mqtt_username: String,
    /// 平台 dev_gateway.mqtt_secret
    pub mqtt_password: String,
    /// 平台网关配置同步 API 根地址，例如 http://host:8103。
    pub platform_http_url: String,
    /// 心跳周期（秒）；平台在连续缺失 3 个周期后判离线。
    pub heartbeat_s: u64,
    /// 真实采样归批周期（秒），默认 5 分钟。
    pub report_interval_s: u64,
    /// 默认采集周期（秒）
    pub data_interval_s: u64,
}

impl Default for CloudSection {
    fn default() -> Self {
        Self {
            gateway_id: "1".to_string(),
            gateway_sn: "GW-DEMO-001".to_string(),
            mqtt_host: "127.0.0.1".to_string(),
            mqtt_port: 1883,
            mqtt_username: String::new(),
            mqtt_password: String::new(),
            platform_http_url: "http://127.0.0.1:8103".to_string(),
            heartbeat_s: 30,
            report_interval_s: 300,
            data_interval_s: 300,
        }
    }
}

/// 串口参数（真驱动使用；与 gw-collector::SerialConfig 字段对齐）。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SerialSection {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,
}

impl Default for SerialSection {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "NONE".to_string(),
        }
    }
}

/// TOML 中的电表种子（首次启动时若库中无同 SN 电表则插入）。
#[derive(Debug, Clone, Deserialize)]
pub struct MeterSeed {
    pub device_sn: String,
    #[serde(default)]
    pub device_name: String,
    pub modbus_addr: u8,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_interval")]
    pub collect_interval_s: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_channel")]
    pub channel_id: String,
    /// 引导文件属于受控部署配置，可显式允许正式上报。
    #[serde(default = "default_true")]
    pub upload_enabled: bool,
}

fn default_profile() -> String {
    "PD666-3S3".to_string()
}
fn default_interval() -> u64 {
    300
}
fn default_true() -> bool {
    true
}
fn default_channel() -> String {
    "rs485-1".to_string()
}

/// 引导配置根。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BootstrapConfig {
    pub cloud: CloudSection,
    pub serial: SerialSection,
    /// 时区偏移秒（默认 UTC+8）
    pub tz_offset_s: i32,
    /// SQLite 路径
    pub db_path: String,
    /// 寄存器档案目录
    pub profiles_dir: String,
    pub meter: Vec<MeterSeed>,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            cloud: CloudSection::default(),
            serial: SerialSection::default(),
            tz_offset_s: 8 * 3600,
            db_path: "/var/lib/park-gateway/gateway.db".to_string(),
            profiles_dir: "profiles".to_string(),
            meter: Vec::new(),
        }
    }
}

impl BootstrapConfig {
    pub fn from_toml_str(text: &str) -> anyhow::Result<Self> {
        let config: BootstrapConfig = toml::from_str(text)?;
        Ok(config)
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取引导配置 {} 失败: {e}", path.display()))?;
        Self::from_toml_str(&text)
    }
}
