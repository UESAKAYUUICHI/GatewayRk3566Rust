use super::*;
pub(super) fn serial_port_inventory() -> Vec<serde_json::Value> {
    tokio_serial::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| {
            serde_json::json!(
                { "portKey" : port.port_name, "systemPath" : port.port_name, "portType" :
                format!("{:?}", port.port_type), "available" : true }
            )
        })
        .collect()
}
pub(super) fn validate_platform_channels(
    channels: &[gw_store::SyncedChannel],
) -> Vec<(String, Result<(), String>)> {
    channels
        .iter()
        .map(|channel| {
            let result = if !channel.enabled {
                Ok(())
            } else if channel.protocol.trim().to_ascii_uppercase() != "MODBUS_RTU" {
                Err(format!("暂不支持通道协议 {}", channel.protocol))
            } else {
                let port = channel.serial_port.trim();
                if port.is_empty() {
                    Err("平台未下发串口路径".to_string())
                } else if let Some(address) = port.strip_prefix("rtu+tcp://") {
                    address.parse::<std::net::SocketAddr>()
                        .map(|_| ()).map_err(|_| "RTU TCP 地址必须为 IP:端口".to_string())
                } else if !FsPath::new(port).exists() && !serial_port_inventory().iter().any(|item|
                    item.get("systemPath").and_then(|value| value.as_str())
                        .is_some_and(|path| path.eq_ignore_ascii_case(port))) {
                    let inventory = serial_port_inventory()
                        .into_iter()
                        .filter_map(|item| {
                            item.get("systemPath")
                                .and_then(|value| value.as_str())
                                .map(str::to_string)
                        })
                        .collect::<Vec<_>>();
                    let hint = if inventory.is_empty() {
                        "当前系统未枚举到可用串口".to_string()
                    } else {
                        format!("当前可用串口: {}", inventory.join(", "))
                    };
                    Err(format!("平台下发串口 {port} 不存在，{hint}"))
                } else {
                    Ok(())
                }
            };
            (channel.channel_id.clone(), result)
        })
        .collect()
}
pub(super) fn sync_ack_resources(
    payload: &SyncPayload,
    channel_results: &[(String, Result<(), String>)],
) -> Vec<serde_json::Value> {
    let mut resources = Vec::new();
    for channel in &payload.channels {
        let result = channel_results.iter().find(|(id, _)| id == &channel.channel_id);
        let error = result.and_then(|(_, value)| value.as_ref().err()).cloned();
        resources
            .push(
                serde_json::json!(
                    { "resourceType" : "CHANNEL", "resourceKey" : channel.channel_id,
                    "status" : if error.is_some() { "FAILED" } else { "APPLIED" },
                    "message" : error.unwrap_or_else(|| format!("{} {} {}{}{}", channel
                    .serial_port, channel.baud_rate, channel.data_bits, channel.parity,
                    channel.stop_bits)) }
                ),
            );
    }
    for device in &payload.devices {
        resources
            .push(
                serde_json::json!(
                    { "resourceType" : "DEVICE", "resourceKey" : device.device_sn,
                    "status" : "APPLIED", "message" : format!("{} 地址 {}", device
                    .channel_id, device.modbus_addr) }
                ),
            );
    }
    for model in &payload.models {
        resources
            .push(
                serde_json::json!(
                    { "resourceType" : "MODEL", "resourceKey" : format!("{}:{}", model
                    .profile_key, model.version), "status" : "APPLIED", "message" :
                    format!("{} 个读块 / {} 个字段 / {} 个测点", model
                    .read_blocks.len(), model.fields.len(), model.bindings.len()) }
                ),
            );
        for binding in &model.bindings {
            let field = model
                .fields
                .iter()
                .find(|item| item.id == binding.protocol_field_id);
            let block = field
                .and_then(|item| {
                    model.read_blocks.iter().find(|block| block.id == item.read_block_id)
                });
            resources
                .push(
                    serde_json::json!(
                        { "resourceType" : "POINT", "resourceKey" : format!("{}:{}:{}",
                        model.profile_key, model.version, binding.point_code), "status" :
                        "APPLIED", "message" : match (field, block) { (Some(field),
                        Some(block)) => format!("FC{} @{}+{} x{} {}", block
                        .function_code, block.start_address, field.register_offset, field
                        .register_length, field.byte_order), _ => "绑定引用缺失"
                        .to_string() } }
                    ),
                );
        }
    }
    resources
}
