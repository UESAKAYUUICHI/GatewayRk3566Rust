//! MQTT 主题构造。云侧 `MqttTopicParser` 使用正则
//! `^gateway/([^/]+)/(data/upload|status/heartbeat|cmd/response)$` 解析上行主题，
//! 下行指令固定为 `gateway/{id}/cmd/down`。

/// 网关心跳上行主题
pub fn heartbeat_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/status/heartbeat")
}

/// 数据上报上行主题
pub fn data_upload_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/data/upload")
}

/// 指令回执上行主题
pub fn command_response_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/cmd/response")
}

pub fn alarm_upload_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/alarm/up")
}

pub fn alarm_ack_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/alarm/ack")
}

/// 指令下行主题（网关订阅）
pub fn command_down_topic(gateway_id: &str) -> String {
    format!("gateway/{gateway_id}/cmd/down")
}

/// 从指令下行主题中提取网关 ID（与云侧正则保持一致的宽容度）
pub fn gateway_id_of_command_down(topic: &str) -> Option<&str> {
    let rest = topic.strip_prefix("gateway/")?;
    let id = rest.strip_suffix("/cmd/down")?;
    if id.is_empty() || id.contains('/') {
        return None;
    }
    Some(id)
}
