use super::*;
/// 指令执行任务：消费云链路下行。
pub(super) async fn commander_task(
    ctx: TaskCtx,
    reboot: Arc<dyn RebootHook>,
    mut command_rx: mpsc::Receiver<RawCommand>,
    shutdown: CancellationToken,
) {
    loop {
        let raw = tokio::select! {
            _ = shutdown.cancelled() => return, received = command_rx.recv() => match
            received { Some(raw) => raw, None => return, },
        };
        handle_raw_command(&ctx, &reboot, raw).await;
    }
}
pub(super) async fn handle_raw_command(
    ctx: &TaskCtx,
    reboot: &Arc<dyn RebootHook>,
    raw: RawCommand,
) {
    let now = ctx.now_ms();
    if raw.topic.ends_with("/alarm/config/notify") {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw.payload) {
            if value["schemaVersion"].as_u64() == Some(1) {
                if let Some(revision) = value["desiredRevision"].as_str() {
                    if revision.len() == 64 && revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                        let _ = ctx
                            .store
                            .set_config("alarm_strategy_requested_revision", revision);
                    }
                }
            }
        }
        return;
    }
    if raw.topic.ends_with("/alarm/ack") {
        match serde_json::from_slice::<proto::AlarmAckPayload>(&raw.payload) {
            Ok(ack) => {
                let _ = ctx
                    .store
                    .mark_alarm_cloud_ack(&ack.event_id, &ack.status, now);
                ctx.emitter.emit(
                    if matches!(
                        ack.status.as_str(),
                        "MQTT_ACKED" | "ACCESS_FORWARDED" | "DATA_CONSUMED"
                    ) {
                        "INFO"
                    } else {
                        "WARN"
                    },
                    "alarm-ack",
                    format!("告警 {} 平台回执：{}", ack.event_id, ack.status),
                );
            }
            Err(error) => {
                ctx.emitter
                    .emit("WARN", "alarm-ack", format!("告警回执解析失败: {error}"))
            }
        }
        return;
    }
    let parsed: Result<proto::CommandBody, _> = serde_json::from_slice(&raw.payload);
    let body = match parsed {
        Ok(body) => body,
        Err(e) => {
            ctx.emitter
                .emit("WARN", "command", format!("指令解析失败: {e}"));
            return;
        }
    };
    if body.command_id.is_empty() {
        return;
    }
    if let Ok(Some((status, message))) = ctx.store.command_result(&body.command_id) {
        let response = if status == "SUCCESS" {
            proto::CommandResponse::success(&body.command_id, message, now)
        } else {
            proto::CommandResponse::failed(&body.command_id, message, now)
        };
        let topic = topics::command_response_topic(&ctx.state.gateway_id());
        if let Ok(bytes) = serde_json::to_vec(&response) {
            let _ = ctx.link.publish(&topic, &bytes).await;
        }
        return;
    }
    let action = command::interpret(&body);
    let gateway_id = ctx.state.gateway_id();
    let response = match &action {
        GatewayAction::ReadNow { target_sn } => match ctx.state.meter_by_sn(target_sn) {
            Some(meter) if meter.record.enabled => {
                let rows = ctx
                    .store
                    .register_map(&meter.record.profile)
                    .unwrap_or_default();
                let specs = specs_of(&rows);
                match ctx.transports.get(&meter.record.channel_id) {
                    Some(transport) => match poll_once(&transport, &meter.record, &specs).await {
                        Ok(points) => {
                            handle_sample(ctx, &meter, points).await;
                            proto::CommandResponse::success(&body.command_id, "已读取并出网", now)
                        }
                        Err(e) => proto::CommandResponse::failed(
                            &body.command_id,
                            format!("读取失败: {e}"),
                            now,
                        ),
                    },
                    None => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("RS485 通道 {} 未建立传输", meter.record.channel_id),
                        now,
                    ),
                }
            }
            _ => proto::CommandResponse::failed(
                &body.command_id,
                format!("目标设备不存在或停用: {target_sn}"),
                now,
            ),
        },
        GatewayAction::SetInterval { seconds } => {
            match policy::validate_collect_interval(*seconds) {
                Ok(validated) => {
                    apply_interval(ctx, validated).await;
                    proto::CommandResponse::success(
                        &body.command_id,
                        format!("采集周期已设为 {validated}s"),
                        now,
                    )
                }
                Err(e) => proto::CommandResponse::failed(&body.command_id, e, now),
            }
        }
        GatewayAction::Reboot => {
            let response = proto::CommandResponse::success(&body.command_id, "网关即将重启", now);
            let reboot = reboot.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                reboot.reboot();
            });
            response
        }
        GatewayAction::DeviceCommand {
            target_sn,
            command_code,
        } => {
            if command_code.is_empty() {
                proto::CommandResponse::failed(&body.command_id, "commandCode 不能为空", now)
            } else if let Some(meter) = ctx.state.meter_by_sn(target_sn) {
                match ctx
                    .store
                    .protocol_command(&meter.record.profile, command_code)
                {
                    Ok(Some(command))
                        if command.function_code == 6
                            && command.encode_type.eq_ignore_ascii_case("FIXED")
                            && command.fixed_value.is_some() =>
                    {
                        match ctx.transports.get(&meter.record.channel_id) {
                            Some(transport) => {
                                let mut guard = transport.lock().await;
                                match guard
                                    .write_single_register(
                                        meter.record.modbus_addr,
                                        command.register_address,
                                        command.fixed_value.unwrap_or_default(),
                                    )
                                    .await
                                {
                                    Ok(()) => proto::CommandResponse::success(
                                        &body.command_id,
                                        format!("已执行 {}", command.command_name),
                                        now,
                                    ),
                                    Err(error) => proto::CommandResponse::failed(
                                        &body.command_id,
                                        format!("设备写入失败: {error}"),
                                        now,
                                    ),
                                }
                            }
                            None => proto::CommandResponse::failed(
                                &body.command_id,
                                format!("RS485 通道 {} 未建立传输", meter.record.channel_id),
                                now,
                            ),
                        }
                    }
                    Ok(Some(_)) => proto::CommandResponse::failed(
                        &body.command_id,
                        "该白名单命令的写入类型暂不受支持",
                        now,
                    ),
                    Ok(None) => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("命令未在设备协议白名单中: {command_code}"),
                        now,
                    ),
                    Err(error) => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("命令白名单读取失败: {error}"),
                        now,
                    ),
                }
            } else {
                proto::CommandResponse::failed(
                    &body.command_id,
                    format!("目标设备不存在或停用: {target_sn}"),
                    now,
                )
            }
        }
        GatewayAction::Unknown { kind } => proto::CommandResponse::failed(
            &body.command_id,
            format!("不支持的指令类型: {kind}"),
            now,
        ),
    };
    let _ = ctx.store.record_command(&gw_store::CommandLogRow {
        command_id: body.command_id.clone(),
        command_type: action.name().to_string(),
        target_sn: body.target_sn.clone().unwrap_or_default(),
        payload_json: body.payload.to_string(),
        received_ms: now,
        result_status: response.status.clone(),
        responded_ms: now,
        message: response.message.clone(),
    });
    ctx.emitter.emit(
        "INFO",
        "command",
        format!(
            "指令 {} {} → {}",
            body.command_id,
            action.name(),
            response.status
        ),
    );
    let topic = topics::command_response_topic(&gateway_id);
    if let Ok(bytes) = serde_json::to_vec(&response) {
        if let Err(e) = ctx.link.publish(&topic, &bytes).await {
            tracing::warn!("指令回执发布失败 {}: {e}", body.command_id);
        }
    }
}
/// SET_INTERVAL：全局采集周期（所有表 + 默认值）。
pub(super) async fn apply_interval(ctx: &TaskCtx, seconds: u64) {
    let _ = ctx
        .store
        .set_config("data_interval_s", &seconds.to_string());
    for meter in ctx.state.meters_snapshot() {
        let mut input = gw_store::MeterInput {
            device_sn: meter.record.device_sn.clone(),
            device_name: meter.record.device_name.clone(),
            modbus_addr: meter.record.modbus_addr,
            profile: meter.record.profile.clone(),
            channel_id: meter.record.channel_id.clone(),
            upload_enabled: meter.record.upload_enabled,
            collect_interval_s: seconds,
            enabled: meter.record.enabled,
        };
        input.collect_interval_s = seconds;
        let _ = ctx.store.update_meter(meter.record.id, &input);
        ctx.state.update_meter(meter.record.id, |m| {
            m.record.collect_interval_s = seconds;
        });
    }
    ctx.emitter.emit(
        "INFO",
        "command",
        format!("全部电表采集周期已调整为 {seconds}s"),
    );
}
