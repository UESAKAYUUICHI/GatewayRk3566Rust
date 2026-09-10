//! 与 park-energy-access 严格对齐的报文协议模型。
//!
//! 字段命名与云侧 `GatewayUploadPayload` / `MeterPayload` / `CommandResponsePayload`
//! 一一对应（camelCase）：
//! 云侧解析规则 —— `@JsonIgnoreProperties(ignoreUnknown = true)` + camelCase 为主、
//! snake_case 为别名；messageId 是幂等键（gatewayId + messageId）。

pub mod ids;
pub mod model;
pub mod points;
pub mod topics;

pub use model::{
    AlarmAckPayload, CommandBody, CommandResponse, DeviceHeartbeat, GatewayAlarmPayload,
    GatewayUploadPayload, HeartbeatPayload, MeterSample,
};
