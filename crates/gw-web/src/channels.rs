use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChannelDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) protocol: String,
    pub(super) port: String,
    pub(super) baud: u32,
    pub(super) data_bits: u8,
    pub(super) stop_bits: u8,
    pub(super) parity: String,
    pub(super) timeout_ms: u64,
    pub(super) retry_count: u32,
    pub(super) poll_interval_seconds: u64,
    pub(super) config_source: String,
    pub(super) enabled: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChannelRequest {
    pub(super) name: String,
    pub(super) port: String,
    pub(super) baud: u32,
    pub(super) data_bits: u8,
    pub(super) stop_bits: u8,
    pub(super) parity: String,
    #[serde(default = "default_timeout_ms")]
    pub(super) timeout_ms: u64,
    #[serde(default = "default_retry_count")]
    pub(super) retry_count: u32,
    #[serde(default = "default_poll_interval_seconds")]
    pub(super) poll_interval_seconds: u64,
    pub(super) enabled: bool,
}
pub(super) async fn channels(
    State(state): State<WebState>,
) -> ApiResult<Vec<ChannelDto>> {
    Ok(
        Json(
            state
                .store
                .channels()
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .map(|c| ChannelDto {
                    id: c.id,
                    name: c.name,
                    protocol: c.protocol,
                    port: c.port,
                    baud: c.baud,
                    data_bits: c.data_bits,
                    stop_bits: c.stop_bits,
                    parity: c.parity,
                    timeout_ms: c.timeout_ms,
                    retry_count: c.retry_count,
                    poll_interval_seconds: c.poll_interval_s,
                    config_source: c.config_source,
                    enabled: c.enabled,
                })
                .collect(),
        ),
    )
}
pub(super) async fn create_channel(
    State(state): State<WebState>,
    Json(body): Json<ChannelRequest>,
) -> ApiResult<ChannelDto> {
    let id = next_channel_id(&state)?;
    let channel = channel_record(id, body)?;
    state
        .store
        .upsert_channel(&channel, now_ms())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    refresh_channel_transport(&state, &channel).await?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(channel_dto(channel)))
}
pub(super) async fn save_channel(
    Path(id): Path<String>,
    State(state): State<WebState>,
    Json(body): Json<ChannelRequest>,
) -> ApiResult<Accepted> {
    let channel = channel_record(id, body)?;
    state
        .store
        .upsert_channel(&channel, now_ms())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    refresh_channel_transport(&state, &channel).await?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn delete_channel(
    Path(id): Path<String>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(ApiError::bad_request("请选择要删除的 RS485 通道"));
    }
    state
        .store
        .delete_channel_with_meters(&id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    state.transports.remove(&id);
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) fn next_channel_id(state: &WebState) -> Result<String, ApiError> {
    let channels = state
        .store
        .channels()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut next = 1;
    loop {
        let id = format!("rs485-{next}");
        if channels.iter().all(|channel| channel.id != id) {
            return Ok(id);
        }
        next += 1;
    }
}
pub(super) fn channel_record(
    id: String,
    body: ChannelRequest,
) -> Result<gw_store::Rs485ChannelRecord, ApiError> {
    let id = id.trim().to_string();
    let port = body.port.trim().to_string();
    if id.is_empty() || port.is_empty() || body.baud == 0 {
        return Err(
            ApiError::bad_request("通道编号、串口和波特率不能为空"),
        );
    }
    if !(5..=8).contains(&body.data_bits) {
        return Err(ApiError::bad_request("数据位必须在 5-8 之间"));
    }
    if !(1..=2).contains(&body.stop_bits) {
        return Err(ApiError::bad_request("停止位必须是 1 或 2"));
    }
    let parity = body.parity.trim().to_ascii_uppercase();
    if !matches!(parity.as_str(), "NONE" | "EVEN" | "ODD") {
        return Err(ApiError::bad_request("校验位必须是 NONE、EVEN 或 ODD"));
    }
    if !(100..=60_000).contains(&body.timeout_ms) || body.retry_count > 10
        || !(5..=86_400).contains(&body.poll_interval_seconds)
    {
        return Err(
            ApiError::bad_request(
                "超时、重试或默认采集周期超出允许范围",
            ),
        );
    }
    Ok(gw_store::Rs485ChannelRecord {
        id,
        name: if body.name.trim().is_empty() {
            "RS485".into()
        } else {
            body.name.trim().to_string()
        },
        protocol: "MODBUS_RTU".into(),
        port,
        baud: body.baud,
        data_bits: body.data_bits,
        stop_bits: body.stop_bits,
        parity,
        timeout_ms: body.timeout_ms,
        retry_count: body.retry_count,
        poll_interval_s: body.poll_interval_seconds,
        config_source: "LOCAL".into(),
        enabled: body.enabled,
    })
}
pub(super) fn channel_dto(channel: gw_store::Rs485ChannelRecord) -> ChannelDto {
    ChannelDto {
        id: channel.id,
        name: channel.name,
        protocol: channel.protocol,
        port: channel.port,
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity,
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
        poll_interval_seconds: channel.poll_interval_s,
        config_source: channel.config_source,
        enabled: channel.enabled,
    }
}
pub(super) async fn refresh_channel_transport(
    state: &WebState,
    channel: &gw_store::Rs485ChannelRecord,
) -> Result<(), ApiError> {
    state.transports.remove(&channel.id);
    if !channel.enabled {
        return Ok(());
    }
    if channel.protocol != "MODBUS_RTU" {
        return Err(
            ApiError::bad_request(
                format!("暂不支持通道协议 {}", channel.protocol),
            ),
        );
    }
    let serial = gw_collector::SerialConfig {
        port: channel.port.clone(),
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity.clone(),
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
    };
    match gw_collector::ModbusRtuTransport::connect(&serial).await {
        Ok(transport) => {
            state
                .transports
                .insert(
                    channel.id.clone(),
                    Arc::new(tokio::sync::Mutex::new(transport)),
                );
            Ok(())
        }
        Err(error) => {
            Err(
                ApiError::unavailable(
                    format!(
                        "通道 {} 无法打开串口 {}: {error}", channel.id, channel
                        .port
                    ),
                ),
            )
        }
    }
}
pub(super) async fn rebuild_channel_transports(
    state: &WebState,
) -> Vec<(String, Result<(), String>)> {
    for channel_id in state.transports.channel_ids() {
        state.transports.remove(&channel_id);
    }
    let channels = match state.store.channels() {
        Ok(channels) => channels,
        Err(error) => return vec![("__store__".into(), Err(error.to_string()))],
    };
    let mut results = Vec::new();
    for channel in channels.into_iter().filter(|channel| channel.enabled) {
        let channel_id = channel.id.clone();
        let result = refresh_channel_transport(state, &channel)
            .await
            .map_err(|error| error.message);
        results.push((channel_id, result));
    }
    results
}
