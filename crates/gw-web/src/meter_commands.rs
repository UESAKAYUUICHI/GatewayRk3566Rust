use super::*;
pub(super) async fn meters(State(state): State<WebState>) -> ApiResult<Vec<MeterDto>> {
    Ok(Json(build_meter_list(&state.snapshot_rx.borrow(), &state.store)?))
}
pub(super) async fn create_meter(
    State(state): State<WebState>,
    Json(body): Json<MeterRequest>,
) -> Result<(StatusCode, Json<Accepted>), ApiError> {
    let mut input = meter_input(body);
    input.upload_enabled = false;
    validate_local_meter(&state, None, &input)?;
    send_command(
            &state,
            UserCommand::SaveMeter {
                id: None,
                input,
            },
        )
        .await?;
    Ok((StatusCode::ACCEPTED, Json(Accepted { accepted: true })))
}
pub(super) async fn update_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<MeterRequest>,
) -> ApiResult<Accepted> {
    let mut input = meter_input(body);
    let existing = state
        .store
        .meters()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .find(|meter| meter.id == id)
        .ok_or_else(|| ApiError::bad_request("设备不存在"))?;
    if existing.config_source == "PLATFORM" {
        return Err(
            ApiError::bad_request(
                "线上设备以平台为准，请在平台修改后重新同步",
            ),
        );
    }
    input.upload_enabled = false;
    validate_local_meter(&state, Some(id), &input)?;
    send_command(
            &state,
            UserCommand::SaveMeter {
                id: Some(id),
                input,
            },
        )
        .await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) fn validate_local_meter(
    state: &WebState,
    editing_id: Option<i64>,
    input: &gw_store::MeterInput,
) -> Result<(), ApiError> {
    if !(1..=247).contains(&input.modbus_addr) {
        return Err(ApiError::bad_request("Modbus 地址必须在 1-247 之间"));
    }
    let channel = state
        .store
        .channels()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .find(|channel| channel.id == input.channel_id)
        .ok_or_else(|| ApiError::bad_request("请选择已存在的 RS485 通道"))?;
    if input.enabled && !channel.enabled {
        return Err(
            ApiError::bad_request("该 RS485 通道已停用，不能启用设备"),
        );
    }
    if input.enabled {
        let conflict = state
            .store
            .meters()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .find(|meter| {
                meter.enabled && meter.channel_id == input.channel_id
                    && meter.modbus_addr == input.modbus_addr
                    && editing_id != Some(meter.id)
            });
        if let Some(meter) = conflict {
            return Err(
                ApiError::bad_request(
                    format!(
                        "同一 RS485 通道内 Modbus 地址不能重复：{} 地址 {} 已被 {} 使用",
                        input.channel_id, input.modbus_addr, meter.device_sn
                    ),
                ),
            );
        }
    }
    if state
        .store
        .register_map(&input.profile)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_empty()
    {
        return Err(ApiError::bad_request("请选择已配置测点的物模型"));
    }
    Ok(())
}
pub(super) async fn delete_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    send_command(&state, UserCommand::DeleteMeter { id }).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn bind_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<BindMeterRequest>,
) -> ApiResult<Accepted> {
    state
        .store
        .bind_meter_to_channel(id, &body.channel_id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn read_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    send_command(
            &state,
            UserCommand::ReadNow {
                meter_id: id,
            },
        )
        .await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn set_continuous_pull(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<ContinuousPullRequest>,
) -> ApiResult<Accepted> {
    send_command(
            &state,
            UserCommand::SetContinuousPull {
                meter_id: id,
                enabled: body.enabled,
            },
        )
        .await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) fn meter_input(body: MeterRequest) -> MeterInput {
    MeterInput {
        device_sn: body.device_sn,
        device_name: body.device_name,
        modbus_addr: body.modbus_addr,
        profile: body.profile,
        channel_id: body.channel_id,
        upload_enabled: body.upload_enabled,
        collect_interval_s: body.collect_interval_s,
        enabled: body.enabled,
    }
}
