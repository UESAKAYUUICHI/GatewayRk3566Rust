use super::*;
/// 档案行 → 解码规格。
pub(super) fn specs_of(
    rows: &[gw_store::RegisterMapRow],
) -> Vec<gw_store::RegisterMapRow> {
    rows.iter()
        .filter(|row| {
            row.data_type.eq_ignore_ascii_case("boolean")
                || DataType::parse(&row.data_type).is_some()
        })
        .cloned()
        .collect()
}
/// Read each block once, then split fields and map them to canonical point codes.
pub(super) async fn poll_once<T: MeterTransport + ?Sized>(
    transport: &Mutex<T>,
    meter: &MeterRecord,
    specs: &[gw_store::RegisterMapRow],
) -> Result<Vec<(String, f64)>, gw_collector::TransportError> {
    let mut guard = transport.lock().await;
    let mut words: HashMap<(u8, u16, u16), Vec<u16>> = HashMap::new();
    for spec in specs {
        let key = (spec.func, spec.address, spec.quantity);
        if let std::collections::hash_map::Entry::Vacant(entry) = words.entry(key) {
            let got = guard
                .read_registers(
                    spec.func,
                    meter.modbus_addr,
                    spec.address,
                    spec.quantity,
                )
                .await?;
            entry.insert(got);
        }
    }
    let mut points: Vec<(String, f64)> = Vec::with_capacity(specs.len());
    for spec in specs {
        let Some(block) = words.get(&(spec.func, spec.address, spec.quantity)) else {
            continue;
        };
        let start = spec.field_offset as usize;
        let end = start + spec.field_quantity as usize;
        if end > block.len() {
            continue;
        }
        let value = if let Some(bit_offset) = spec.bit_offset {
            let width = spec.bit_length.unwrap_or(1).min(16);
            let mask = if width == 16 { u16::MAX } else { (1u16 << width) - 1 };
            ((block[start] >> bit_offset) & mask) as f64 * spec.scale + spec.offset
        } else {
            let Some(data_type) = DataType::parse(&spec.data_type) else {
                continue;
            };
            let field = RegisterSpec {
                point: spec.point_code.clone(),
                func: spec.func,
                address: 0,
                quantity: spec.field_quantity,
                data_type,
                byte_order: spec.byte_order.clone(),
                scale: spec.scale,
                offset: spec.offset,
            };
            MeterProfile::decode_one(&field, &block[start..end])
        };
        points.push((spec.point_code.clone(), value));
    }
    Ok(points)
}
pub(super) async fn ensure_channel_transport(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
) -> Option<crate::transport_registry::TransportHandle> {
    if let Some(transport) = ctx.transports.get(&meter.record.channel_id) {
        return Some(transport);
    }
    let channel = match ctx.store.channels() {
        Ok(channels) => {
            channels
                .into_iter()
                .find(|channel| channel.id == meter.record.channel_id && channel.enabled)
        }
        Err(error) => {
            ctx.emitter
                .emit(
                    "WARN",
                    "poller",
                    format!("读取 RS485 通道配置失败: {error}"),
                );
            None
        }
    }?;
    let protocol = channel.protocol.trim().to_ascii_uppercase();
    if protocol != "MODBUS_RTU" {
        ctx.emitter
            .emit(
                "WARN",
                "poller",
                format!("通道 {} 协议 {protocol} 暂不支持", channel.id),
            );
        return None;
    }
    let serial = SerialConfig {
        port: channel.port.clone(),
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity.clone(),
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
    };
    match ModbusRtuTransport::connect(&serial).await {
        Ok(transport) => {
            let handle = Arc::new(Mutex::new(transport));
            ctx.transports.insert(channel.id.clone(), handle.clone());
            ctx.emitter
                .emit(
                    "INFO",
                    "poller",
                    format!(
                        "RS485 通道 {} 已自动恢复: {}", channel.id, channel.port
                    ),
                );
            Some(handle)
        }
        Err(error) => {
            ctx.emitter
                .emit(
                    "WARN",
                    "poller",
                    format!("RS485 通道 {} 自动恢复失败: {error}", channel.id),
                );
            None
        }
    }
}
