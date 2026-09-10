//! 存储层数据记录类型（与表结构一一对应）。

/// 电表档案行。
#[derive(Debug, Clone, PartialEq)]
pub struct MeterRecord {
    pub id: i64,
    pub device_sn: String,
    pub device_name: String,
    pub modbus_addr: u8,
    pub profile: String,
    pub channel_id: String,
    /// LOCAL | PLATFORM
    pub config_source: String,
    pub platform_device_id: Option<i64>,
    pub model_version: String,
    /// 仅平台注册设备允许进入正式业务上报。
    pub upload_enabled: bool,
    pub collect_interval_s: u64,
    pub enabled: bool,
    pub created_ms: u64,
}

/// 新建/更新电表的输入。
#[derive(Debug, Clone, PartialEq)]
pub struct MeterInput {
    pub device_sn: String,
    pub device_name: String,
    pub modbus_addr: u8,
    pub profile: String,
    pub channel_id: String,
    pub upload_enabled: bool,
    pub collect_interval_s: u64,
    pub enabled: bool,
}

/// 寄存器映射行（data_type: "u16" | "i16" | "u32" | "i32" | "f32"）。
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterMapRow {
    pub profile: String,
    pub point_code: String,
    pub point_name: String,
    pub unit: String,
    /// Modbus 功能码（3 = 读保持寄存器）
    pub func: u8,
    pub address: u16,
    pub quantity: u16,
    pub data_type: String,
    /// Register byte/word order, for example ABCD/BADC/CDAB/DCBA.
    pub byte_order: String,
    pub scale: f64,
    pub offset: f64,
}

/// 每次真实采样的历史行；在成功归入 outbox 前一直保持 `outbox_id = NULL`。
#[derive(Debug, Clone)]
pub struct SampleRecord {
    pub id: i64,
    pub meter_id: i64,
    pub device_sn: String,
    pub modbus_addr: u8,
    pub channel_id: String,
    pub profile: String,
    pub model_version: String,
    pub config_revision: Option<String>,
    pub read_ms: u64,
    /// Actual interval in effect when this row was collected.
    pub sample_interval_s: u64,
    pub quality: u32,
    pub points_json: String,
}

/// RS485 物理通道。设备必须绑定通道后才允许采集。
#[derive(Debug, Clone, PartialEq)]
pub struct Rs485ChannelRecord {
    pub id: String,
    pub name: String,
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,
    pub enabled: bool,
}

/// 本地告警事件。source: GATEWAY | PLATFORM；source_event_id 用于跨端去重。
#[derive(Debug, Clone)]
pub struct AlarmEventRecord {
    pub source_event_id: String,
    pub source: String,
    pub meter_id: Option<i64>,
    pub alarm_type: String,
    pub level: String,
    pub point_code: Option<String>,
    pub message: String,
    pub status: String,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// 网关告警的北向处理状态；平台下行告警固定为 SYNCED。
    pub cloud_status: String,
    pub cloud_message_id: Option<String>,
    pub cloud_ack_ms: Option<u64>,
}

/// 告警规则。source: PLATFORM 表示平台下发、网关侧锁定只读；LOCAL 表示网关本地规则。
#[derive(Debug, Clone)]
pub struct AlarmRuleRecord {
    pub id: i64,
    pub rule_code: String,
    pub source: String,
    pub name: String,
    pub level: String,
    pub target_device_sn: Option<String>,
    pub point_code: String,
    pub operator: String,
    pub threshold: f64,
    pub unit: String,
    pub duration_s: u64,
    pub enabled: bool,
    pub updated_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SyncedThingModel {
    pub profile: String,
    pub name: String,
    pub version: String,
    pub platform_model_id: i64,
    pub points: Vec<RegisterMapRow>,
}

#[derive(Debug, Clone)]
pub struct SyncedDevice {
    pub platform_device_id: i64,
    pub device_sn: String,
    pub device_name: String,
    pub modbus_addr: u8,
    pub profile: String,
    pub channel_id: String,
    pub model_version: String,
    pub collect_interval_s: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ThingModelSummary {
    pub profile: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub point_count: u32,
    pub device_count: u32,
}

/// 待入 outbox 的报文。
#[derive(Debug, Clone)]
pub struct NewOutbox {
    pub message_id: String,
    pub topic: String,
    pub payload: Vec<u8>,
    pub created_ms: u64,
}

/// outbox 行（发布器视角）。
#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub id: i64,
    pub message_id: String,
    pub topic: String,
    pub payload: Vec<u8>,
    pub created_ms: u64,
    pub attempts: u32,
}

/// 最新读数快照。
#[derive(Debug, Clone)]
pub struct ReadingRecord {
    pub meter_id: i64,
    /// 上报 points 的 JSON 串
    pub snapshot_json: String,
    pub read_ms: u64,
    pub sample_interval_s: u64,
    pub quality: u32,
}

/// 指令日志行。
#[derive(Debug, Clone)]
pub struct CommandLogRow {
    pub command_id: String,
    pub command_type: String,
    pub target_sn: String,
    pub payload_json: String,
    pub received_ms: u64,
    pub result_status: String,
    pub responded_ms: u64,
    pub message: String,
}

/// 事件日志行。
#[derive(Debug, Clone)]
pub struct EventRow {
    pub ts_ms: u64,
    /// INFO | WARN | ERROR
    pub level: String,
    pub source: String,
    pub message: String,
}
