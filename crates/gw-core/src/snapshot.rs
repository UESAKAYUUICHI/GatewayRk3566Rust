//! UI / 外部观察者可见的系统快照模型。
//! Agent 以约 1Hz 频率发布快照，UI 层只读不问，避免事件风暴。

use serde::Serialize;

/// 云链路健康状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkHealth {
    Connected,
    Connecting,
    Down,
}

impl LinkHealth {
    pub fn label(&self) -> &'static str {
        match self {
            LinkHealth::Connected => "已连接",
            LinkHealth::Connecting => "连接中",
            LinkHealth::Down => "已断开",
        }
    }

    pub fn is_publishable(&self) -> bool {
        matches!(self, LinkHealth::Connected)
    }
}

/// 单表实时快照。
#[derive(Debug, Clone, Serialize)]
pub struct MeterSnapshot {
    pub id: i64,
    pub device_sn: String,
    pub online: bool,
    /// Modbus 从站地址
    pub addr: u8,
    /// 采集周期（秒）
    pub interval_s: u64,
    /// 是否处于设备监控页发起的每 5 秒持续拉取状态
    pub continuous_pull: bool,
    pub enabled: bool,
    /// 本地寄存器档案名，可在 UI 中切换为新增表型的 profile。
    pub profile: String,
    pub last_read_ms: u64,
    pub quality: u32,
    /// 点位值（键与上报报文一致）
    pub points: Vec<(String, f64)>,
    /// 今日用电（kWh，本地统计）
    pub today_kwh: f64,
}

/// 系统级快照（watch 通道载荷）。
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub cloud: LinkHealth,
    pub clock_trusted: bool,
    pub outbox_pending: u64,
    pub outbox_sent_total: u64,
    pub meters_online: u32,
    pub meters_total: u32,
    pub today_kwh_total: f64,
    pub last_publish_ms: u64,
    pub last_heartbeat_ms: u64,
    pub started_ms: u64,
    pub version: String,
    pub meters: Vec<MeterSnapshot>,
}

impl Snapshot {
    pub fn uptime_s(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.started_ms) / 1000
    }
}

/// 事件行（日志环 + 落库）。
#[derive(Debug, Clone, Serialize)]
pub struct EventLine {
    pub ts_ms: u64,
    pub level: String,
    pub source: String,
    pub message: String,
}
