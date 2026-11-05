use super::*;
/// UI 命令处理任务。
pub(super) async fn user_command_task(
    ctx: TaskCtx,
    mut cmd_rx: mpsc::Receiver<crate::ui_command::UserCommand>,
    shutdown: CancellationToken,
) {
    loop {
        let command = tokio::select! {
            _ = shutdown.cancelled() => return, received = cmd_rx.recv() => match
            received { Some(command) => command, None => return, },
        };
        use crate::ui_command::UserCommand;
        match command {
            UserCommand::SaveLink {
                gateway_id,
                gateway_sn,
                mqtt_host,
                mqtt_port,
                mqtt_username,
                mqtt_password,
            } => {
                let _ = ctx.store.set_config("mqtt_host", &mqtt_host);
                let _ = ctx.store.set_config("mqtt_port", &mqtt_port.to_string());
                let _ = ctx.store.set_config("mqtt_username", &mqtt_username);
                let _ = ctx.store.set_config("mqtt_password", &mqtt_password);
                let _ = ctx.store.set_config("gateway_id", &gateway_id);
                let _ = ctx.store.set_config("gateway_sn", &gateway_sn);
                ctx.state.set_identity(gateway_id, gateway_sn);
                ctx.emitter
                    .emit(
                        "INFO",
                        "config",
                        "云端参数已保存；broker 连接参数重启服务后生效",
                    );
            }
            UserCommand::SetHeartbeat { seconds } => {
                match policy::validate_heartbeat_interval(seconds) {
                    Ok(validated) => {
                        let _ = ctx
                            .store
                            .set_config("heartbeat_s", &validated.to_string());
                        ctx.state.set_heartbeat_s(validated);
                        ctx.emitter
                            .emit(
                                "INFO",
                                "config",
                                format!("心跳周期已设为 {validated}s"),
                            );
                    }
                    Err(e) => ctx.emitter.emit("WARN", "config", e),
                }
            }
            UserCommand::SaveMeter { id, input } => {
                if let Err(e) = policy::validate_device_sn(&input.device_sn)
                    .and_then(|_| {
                        policy::validate_collect_interval(input.collect_interval_s)
                            .map(|_| ())
                    })
                {
                    ctx.emitter
                        .emit("WARN", "config", format!("电表参数非法: {e}"));
                    continue;
                }
                let result = match id {
                    Some(id) => ctx.store.update_meter(id, &input).map(|_| id),
                    None => ctx.store.insert_meter(&input, ctx.now_ms()),
                };
                match result {
                    Ok(saved_id) => {
                        reload_meter_runtime(&ctx, saved_id);
                        ctx.sync_pollers(&shutdown);
                        ctx.emitter
                            .emit(
                                "INFO",
                                "config",
                                format!("电表 {} 已保存", input.device_sn),
                            );
                    }
                    Err(e) => {
                        ctx.emitter
                            .emit("WARN", "config", format!("电表保存失败: {e}"))
                    }
                }
            }
            UserCommand::DeleteMeter { id } => {
                match ctx.store.delete_meter(id) {
                    Ok(()) => {
                        let meters: Vec<MeterRuntime> = ctx
                            .store
                            .meters()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|record| {
                                let reading = ctx.store.reading(record.id).unwrap_or(None);
                                MeterRuntime::from_record(record, reading)
                            })
                            .collect();
                        ctx.state.replace_meters(meters);
                        ctx.sync_pollers(&shutdown);
                        ctx.emitter.emit("INFO", "config", "电表已删除");
                    }
                    Err(e) => {
                        ctx.emitter.emit("WARN", "config", format!("删除失败: {e}"))
                    }
                }
            }
            UserCommand::ReloadConfiguration => {
                let meters: Vec<MeterRuntime> = ctx
                    .store
                    .meters()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|record| {
                        let reading = ctx.store.reading(record.id).unwrap_or(None);
                        MeterRuntime::from_record(record, reading)
                    })
                    .collect();
                ctx.state.replace_meters(meters);
                for id in ctx.registry.current_ids() {
                    ctx.registry.cancel(id);
                }
                ctx.sync_pollers(&shutdown);
                ctx.emitter
                    .emit("INFO", "sync", "平台物模型与设备配置已应用");
            }
            UserCommand::ReadNow { meter_id } => {
                match ctx.state.meter_by_id(meter_id) {
                    Some(meter) if meter.record.enabled => {
                        let rows = ctx
                            .store
                            .register_map(&meter.record.profile)
                            .unwrap_or_default();
                        let specs = specs_of(&rows);
                        let Some(transport) = ctx
                            .transports
                            .get(&meter.record.channel_id) else {
                            handle_channel_unavailable(&ctx, &meter);
                            continue;
                        };
                        match poll_once(&transport, &meter.record, &specs).await {
                            Ok(points) => {
                                handle_sample(&ctx, &meter, points).await;
                                ctx.emitter
                                    .emit(
                                        "INFO",
                                        "command",
                                        format!("{} 手动读取完成", meter.record.device_sn),
                                    );
                            }
                            Err(e) => {
                                let detail = e.to_string();
                                handle_poll_failure(&ctx, &meter, e);
                                ctx.emitter
                                    .emit(
                                        "WARN",
                                        "command",
                                        format!(
                                            "{} 手动读取失败: {detail}", meter.record.device_sn
                                        ),
                                    );
                            }
                        }
                    }
                    _ => {
                        ctx.emitter
                            .emit("WARN", "command", "目标电表不存在或停用")
                    }
                }
            }
            UserCommand::SetContinuousPull { meter_id, enabled } => {
                let Some(meter) = ctx.state.meter_by_id(meter_id) else {
                    ctx.emitter.emit("WARN", "command", "目标电表不存在");
                    continue;
                };
                if !meter.record.enabled {
                    ctx.emitter.emit("WARN", "command", "目标电表已停用");
                    continue;
                }
                ctx.state.set_continuous_pull(meter_id, enabled);
                ctx.registry.cancel(meter_id);
                ctx.sync_pollers(&shutdown);
                ctx.emitter
                    .emit(
                        "INFO",
                        "command",
                        format!(
                            "{}{}持续拉取", meter.record.device_sn, if enabled {
                            " 已开始每 5 秒" } else { " 已停止" }
                        ),
                    );
            }
            UserCommand::DiagnoseCloud => {
                let health = ctx.link.health();
                let clock_ok = ctx.clock_trusted();
                ctx.emitter
                    .emit(
                        "INFO",
                        "link",
                        format!(
                            "云链路诊断：{}，时钟{}", health.label(), if
                            clock_ok { "可信" } else {
                            "未同步（数据暂存不出网）" }
                        ),
                    );
            }
        }
    }
}
/// 档案变更后从库中重载单表运行时（保留在线状态位）。
pub(super) fn reload_meter_runtime(ctx: &TaskCtx, meter_id: i64) {
    let Some(record) = ctx
        .store
        .meters()
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == meter_id) else {
        return;
    };
    let reading = ctx.store.reading(meter_id).unwrap_or(None);
    let mut reloaded = MeterRuntime::from_record(record, reading);
    if let Some(existing) = ctx.state.meter_by_id(meter_id) {
        reloaded.online = existing.online;
        reloaded.last_total_kwh = existing.last_total_kwh.or(reloaded.last_total_kwh);
        reloaded.last_points = existing.last_points;
        reloaded.last_read_ms = existing.last_read_ms;
    }
    let mut meters = ctx.state.meters_snapshot();
    if let Some(slot) = meters.iter_mut().find(|m| m.record.id == meter_id) {
        *slot = reloaded;
    } else {
        meters.push(reloaded);
    }
    ctx.state.replace_meters(meters);
}
