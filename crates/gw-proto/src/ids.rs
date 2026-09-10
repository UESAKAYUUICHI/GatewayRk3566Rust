//! messageId 构造规则，与云侧幂等规则一致。
//! 云侧以 gatewayId + messageId 做入库幂等，重放必须复用同一 messageId。

/// 数据上报报文 ID：`MSG-{gatewayId}-{ts_ms}`
pub fn data_message_id(gateway_id: &str, ts_ms: u64) -> String {
    format!("MSG-{gateway_id}-{ts_ms}")
}

/// 心跳报文 ID：`HB-{gatewayId}-{ts_ms}`
pub fn heartbeat_message_id(gateway_id: &str, ts_ms: u64) -> String {
    format!("HB-{gateway_id}-{ts_ms}")
}

pub fn alarm_message_id(gateway_id: &str, ts_ms: u64) -> String {
    format!("ALARM-{gateway_id}-{ts_ms}")
}
