use super::*;
pub(super) async fn sync_platform_config(
    State(state): State<WebState>,
) -> ApiResult<SyncResult> {
    let get = |key: &str, fallback: String| {
        state.store.get_config(key).ok().flatten().unwrap_or(fallback)
    };
    let base = get("platform_http_url", state.bootstrap.cloud.platform_http_url.clone());
    let gateway_sn = get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone());
    let secret = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    if secret.is_empty() {
        return Err(
            ApiError::bad_request("未配置网关密钥，不能执行线上同步"),
        );
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let response = client
        .get(format!("{}/api/platform/edge/config", base.trim_end_matches('/')))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .send()
        .await
        .map_err(|e| ApiError::unavailable(
            format!("平台配置同步连接失败: {e}"),
        ))?;
    let status = response.status();
    let envelope: PlatformEnvelope<SyncPayload> = response
        .json()
        .await
        .map_err(|e| ApiError::unavailable(
            format!("平台配置响应无法解析: {e}"),
        ))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(
            ApiError::unavailable(format!("平台拒绝同步: {}", envelope.message)),
        );
    }
    let payload = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回配置数据"))?;
    if payload
        .models
        .iter()
        .any(|model| {
            model.read_blocks.is_empty() || model.fields.is_empty()
                || model.bindings.is_empty()
        })
    {
        let _ = state
            .store
            .record_sync_failure(
                Some(&payload.desired_revision),
                "物模型缺少现场采集点",
                now_ms(),
            );
        return Err(
            ApiError::bad_request(
                "平台物模型缺少 Modbus 采集点，已拒绝应用",
            ),
        );
    }
    let channels = payload
        .channels
        .iter()
        .map(|channel| gw_store::SyncedChannel {
            channel_id: channel.channel_id.clone(),
            channel_name: channel.channel_name.clone(),
            protocol: channel.protocol.clone(),
            serial_port: channel.serial_port.clone(),
            baud_rate: channel.baud_rate,
            data_bits: channel.data_bits,
            stop_bits: channel.stop_bits,
            parity: channel.parity.clone(),
            timeout_ms: channel.timeout_ms,
            retry_count: channel.retry_count,
            poll_interval_s: channel.poll_interval_seconds,
            enabled: channel.enabled,
        })
        .collect::<Vec<_>>();
    let channel_precheck = validate_platform_channels(&channels);
    let channel_precheck_errors = channel_precheck
        .iter()
        .filter_map(|(channel, result)| {
            result.as_ref().err().map(|error| format!("{channel}: {error}"))
        })
        .collect::<Vec<_>>();
    if !channel_precheck_errors.is_empty() {
        let message = channel_precheck_errors.join("; ");
        let _ = state
            .store
            .record_sync_failure(Some(&payload.desired_revision), &message, now_ms());
        let _ = client
            .post(format!("{}/api/platform/edge/config/ack", base.trim_end_matches('/')))
            .header("X-Gateway-Sn", &gateway_sn)
            .header("X-Gateway-Secret", &secret)
            .json(
                &serde_json::json!(
                    { "appliedRevision" : payload.desired_revision, "configChecksum" :
                    payload.config_checksum, "status" : "FAILED", "error" : message,
                    "portInventory" : serial_port_inventory(), "resources" :
                    sync_ack_resources(& payload, & channel_precheck) }
                ),
            )
            .send()
            .await;
        return Err(
            ApiError::unavailable(
                format!("平台配置未应用，已保留本地配置: {message}"),
            ),
        );
    }
    let models = payload
        .models
        .iter()
        .map(|model| gw_store::SyncedThingModel {
            profile: model.profile_key.clone(),
            name: model.model_name.clone(),
            version: model.version.clone(),
            platform_model_id: model.model_version_id,
            points: model
                .bindings
                .iter()
                .filter_map(|binding| {
                    let field = model
                        .fields
                        .iter()
                        .find(|field| field.id == binding.protocol_field_id)?;
                    let block = model
                        .read_blocks
                        .iter()
                        .find(|block| block.id == field.read_block_id)?;
                    Some(gw_store::RegisterMapRow {
                        profile: model.profile_key.clone(),
                        point_code: normalize_point_code(&binding.point_code),
                        point_name: field.field_name.clone(),
                        unit: String::new(),
                        func: block.function_code,
                        address: block.start_address,
                        quantity: block.register_count,
                        field_offset: field.register_offset,
                        field_quantity: field.register_length,
                        bit_offset: field.bit_offset,
                        bit_length: field.bit_length,
                        data_type: field.value_type.to_ascii_lowercase(),
                        byte_order: field.byte_order.trim().to_ascii_uppercase(),
                        scale: field.decode_factor * binding.canonical_factor,
                        offset: field.decode_offset * binding.canonical_factor
                            + binding.canonical_offset,
                    })
                })
                .collect(),
            commands: model
                .commands
                .iter()
                .map(|command| gw_store::ProtocolCommandRow {
                    profile: model.profile_key.clone(),
                    command_code: command.command_code.trim().to_ascii_uppercase(),
                    command_name: command.command_name.clone(),
                    function_code: command.function_code,
                    register_address: command.register_address,
                    encode_type: command.encode_type.trim().to_ascii_uppercase(),
                    fixed_value: command.fixed_value,
                    parameter_json: command
                        .parameter_json
                        .as_ref()
                        .map(ToString::to_string),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let devices = payload
        .devices
        .iter()
        .map(|device| gw_store::SyncedDevice {
            platform_device_id: device.platform_device_id,
            device_sn: device.device_sn.clone(),
            device_name: device.device_name.clone(),
            modbus_addr: device.modbus_addr,
            profile: device.profile_key.clone(),
            channel_id: device.channel_id.clone(),
            model_version: device.model_version.clone(),
            collect_interval_s: device.collect_interval_s,
            enabled: device.enabled,
        })
        .collect::<Vec<_>>();
    state
        .store
        .apply_platform_sync(
            &payload.desired_revision,
            &channels,
            &models,
            &devices,
            now_ms(),
        )
        .map_err(|e| {
            let message = format!("配置冲突或写入失败: {e}");
            let _ = state
                .store
                .record_sync_failure(
                    Some(&payload.desired_revision),
                    &message,
                    now_ms(),
                );
            ApiError::bad_request(message)
        })?;
    let channel_results = rebuild_channel_transports(&state).await;
    let channel_errors = channel_results
        .iter()
        .filter_map(|(channel, result)| {
            result.as_ref().err().map(|error| format!("{channel}: {error}"))
        })
        .collect::<Vec<_>>();
    if !channel_errors.is_empty() {
        let message = channel_errors.join("; ");
        let _ = state
            .store
            .record_sync_failure(Some(&payload.desired_revision), &message, now_ms());
        let resources = sync_ack_resources(&payload, &channel_results);
        let _ = client
            .post(format!("{}/api/platform/edge/config/ack", base.trim_end_matches('/')))
            .header("X-Gateway-Sn", &gateway_sn)
            .header("X-Gateway-Secret", &secret)
            .json(
                &serde_json::json!(
                    { "appliedRevision" : payload.desired_revision, "configChecksum" :
                    payload.config_checksum, "status" : "FAILED", "error" : message,
                    "portInventory" : serial_port_inventory(), "resources" : resources }
                ),
            )
            .send()
            .await;
        return Err(ApiError::unavailable(format!("平台配置未应用: {message}")));
    }
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    state
        .store
        .record_sync_applied(&payload.desired_revision, now_ms())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let ack = client
        .post(format!("{}/api/platform/edge/config/ack", base.trim_end_matches('/')))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .json(
            &serde_json::json!(
                { "appliedRevision" : payload.desired_revision, "configChecksum" :
                payload.config_checksum, "status" : "APPLIED", "portInventory" :
                serial_port_inventory(), "resources" : sync_ack_resources(& payload, &
                channel_results) }
            ),
        )
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false);
    Ok(
        Json(SyncResult {
            applied: true,
            revision: payload.desired_revision,
            channels: channels.len(),
            devices: devices.len(),
            models: models.len(),
            cloud_acknowledged: ack,
        }),
    )
}
