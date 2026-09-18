//! 报文模型。serde 序列化输出与云侧 DTO 字段逐一对应。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: &str = "1.0";
pub const TYPE_DATA_UPLOAD: &str = "DATA_UPLOAD";
pub const TYPE_HEARTBEAT: &str = "HEARTBEAT";
pub const TYPE_ALARM_UPLOAD: &str = "ALARM_UPLOAD";
pub const HEARTBEAT_STATUS_ONLINE: &str = "online";
pub const QUALITY_NORMAL: u32 = 0;
/// 累积量发生回退时保留的原始样本质量。
///
/// 保持为 1 是为了兼容已有的“非零质量为告警”展示逻辑，同时由云侧
/// 将它区分为可追溯但不参与计量统计的回退样本。
pub const QUALITY_ROLLBACK: u32 = 1;
pub const COMMAND_STATUS_SUCCESS: &str = "SUCCESS";
pub const COMMAND_STATUS_FAILED: &str = "FAILED";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAlarmPayload {
    pub schema_version: String,
    pub message_id: String,
    pub gateway_sn: String,
    pub timestamp: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub event_id: String,
    pub action: String,
    pub alarm_type: String,
    pub level: String,
    pub device_sn: Option<String>,
    pub point_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmAckPayload {
    pub message_id: String,
    pub event_id: String,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    pub processed_at: u64,
}

/// 数据上报报文（对应云侧 GatewayUploadPayload）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayUploadPayload {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "gatewaySn")]
    pub gateway_sn: String,
    pub timestamp: u64,
    #[serde(rename = "type")]
    pub kind: String,
    /// Deprecated batch default retained for older Data services. New services read the
    /// actual interval from each meter sample.
    #[serde(rename = "sampleIntervalSeconds", default)]
    pub sample_interval_seconds: u64,
    #[serde(rename = "reportWindowSeconds", default)]
    pub report_window_seconds: u64,
    pub meters: Vec<MeterSample>,
}

impl GatewayUploadPayload {
    pub fn new_data(
        message_id: String,
        gateway_sn: String,
        timestamp: u64,
        report_window_seconds: u64,
        meters: Vec<MeterSample>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            message_id,
            gateway_sn,
            timestamp,
            kind: TYPE_DATA_UPLOAD.to_string(),
            sample_interval_seconds: 0,
            report_window_seconds,
            meters,
        }
    }
}

/// 单表采样（对应云侧 MeterPayload）。points 键为已发布的标准测点编码。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeterSample {
    #[serde(rename = "deviceSn")]
    pub device_sn: String,
    #[serde(rename = "modbusAddr")]
    pub modbus_addr: u8,
    #[serde(rename = "channelId")]
    pub channel_id: String,
    #[serde(rename = "profileKey")]
    pub profile_key: String,
    #[serde(rename = "modelVersion")]
    pub model_version: String,
    #[serde(rename = "configRevision", skip_serializing_if = "Option::is_none")]
    pub config_revision: Option<String>,
    #[serde(rename = "collectTime")]
    pub collect_time: u64,
    #[serde(rename = "sampleIntervalSeconds", default)]
    pub sample_interval_seconds: u64,
    #[serde(default)]
    pub quality: u32,
    pub points: BTreeMap<String, f64>,
}

/// 心跳报文（对应云侧 GatewayHeartbeatPayload）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatPayload {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "gatewaySn")]
    pub gateway_sn: String,
    pub timestamp: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub devices: Vec<DeviceHeartbeat>,
    #[serde(rename = "outboxPending", default)]
    pub outbox_pending: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceHeartbeat {
    #[serde(rename = "deviceSn")]
    pub device_sn: String,
    pub online: bool,
    #[serde(rename = "lastReadTime")]
    pub last_read_time: u64,
    pub quality: u32,
}

impl HeartbeatPayload {
    pub fn new_online(
        message_id: String,
        gateway_sn: String,
        timestamp: u64,
        devices: Vec<DeviceHeartbeat>,
        outbox_pending: u64,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            message_id,
            gateway_sn,
            timestamp,
            kind: TYPE_HEARTBEAT.to_string(),
            status: HEARTBEAT_STATUS_ONLINE.to_string(),
            devices,
            outbox_pending,
        }
    }
}

/// 云侧下发的指令体（CommandService.buildCommandBody）。
/// 解析保持宽容：未知字段忽略，字段尽量可选。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommandBody {
    #[serde(rename = "commandId", alias = "command_id", default)]
    pub command_id: String,
    #[serde(rename = "targetType", alias = "target_type", default)]
    pub target_type: Option<String>,
    #[serde(rename = "targetId", alias = "target_id", default)]
    pub target_id: Option<i64>,
    #[serde(rename = "targetSn", alias = "target_sn", default)]
    pub target_sn: Option<String>,
    #[serde(rename = "commandType", alias = "command_type", default)]
    pub command_type: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// 指令回执（云侧 CommandResponseHandler 只认 commandId + status）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResponse {
    #[serde(rename = "commandId")]
    pub command_id: String,
    pub status: String,
    pub message: String,
    pub timestamp: u64,
}

impl CommandResponse {
    pub fn success(
        command_id: impl Into<String>,
        message: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self::new(command_id, COMMAND_STATUS_SUCCESS, message, timestamp)
    }

    pub fn failed(
        command_id: impl Into<String>,
        message: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self::new(command_id, COMMAND_STATUS_FAILED, message, timestamp)
    }

    fn new(
        command_id: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            command_id: command_id.into(),
            status: status.into(),
            message: message.into(),
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_upload_keeps_the_interval_on_each_sample() {
        let payload = GatewayUploadPayload::new_data(
            "MSG-1".into(),
            "GW-1".into(),
            1_000,
            300,
            vec![MeterSample {
                device_sn: "METER-1".into(),
                modbus_addr: 1,
                channel_id: "rs485-1".into(),
                profile_key: "PD666-3S3".into(),
                model_version: "v1".into(),
                config_revision: Some("rev-1".into()),
                collect_time: 1_000,
                sample_interval_seconds: 5,
                quality: QUALITY_NORMAL,
                points: BTreeMap::new(),
            }],
        );

        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["sampleIntervalSeconds"], 0);
        assert_eq!(json["meters"][0]["sampleIntervalSeconds"], 5);
        assert_eq!(json["meters"][0]["channelId"], "rs485-1");
        assert_eq!(json["meters"][0]["configRevision"], "rev-1");
    }
}
