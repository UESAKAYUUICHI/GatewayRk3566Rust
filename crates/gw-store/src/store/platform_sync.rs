use super::*;
impl Store {
    /// 原子应用平台下发配置。LOCAL 设备不会被同步删除；新平台设备先进入暂存区。
    pub fn apply_platform_sync(
        &self,
        revision: &str,
        channels: &[SyncedChannel],
        models: &[SyncedThingModel],
        devices: &[SyncedDevice],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let mut channel_ids = std::collections::HashSet::new();
        let mut enabled_ports = std::collections::HashSet::new();
        for channel in channels {
            let id = channel.channel_id.trim();
            let protocol = channel.protocol.trim().to_ascii_uppercase();
            let port = channel.serial_port.trim();
            if id.is_empty() || port.is_empty() {
                return Err(
                    StoreError::Validation(
                        "platform channel id and serial port are required".into(),
                    ),
                );
            }
            if protocol != "MODBUS_RTU" {
                return Err(
                    StoreError::Validation(
                        format!(
                            "channel {} uses unsupported protocol {}", channel
                            .channel_id, channel.protocol
                        ),
                    ),
                );
            }
            if !channel_ids.insert(id.to_string()) {
                return Err(StoreError::Validation(format!("duplicate channel id {id}")));
            }
            if channel.enabled && !enabled_ports.insert(port.to_ascii_lowercase()) {
                return Err(
                    StoreError::Validation(
                        format!(
                            "serial port {port} is assigned to more than one enabled channel"
                        ),
                    ),
                );
            }
            if channel.baud_rate == 0 || !(5..=8).contains(&channel.data_bits)
                || !(1..=2).contains(&channel.stop_bits)
                || !(100..=60_000).contains(&channel.timeout_ms)
                || channel.retry_count > 10
                || !(5..=86_400).contains(&channel.poll_interval_s)
            {
                return Err(
                    StoreError::Validation(
                        format!(
                            "channel {} has invalid serial or retry settings", channel
                            .channel_id
                        ),
                    ),
                );
            }
        }
        for channel in channels {
            let parity = normalize_parity(&channel.parity)?;
            tx.execute(
                "INSERT INTO rs485_channel(id,name,protocol,port,baud,data_bits,stop_bits,parity,timeout_ms,
                    retry_count,poll_interval_s,config_source,enabled,updated_ms)
                 VALUES(?1,?2,'MODBUS_RTU',?3,?4,?5,?6,?7,?8,?9,?10,'PLATFORM',?11,?12)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name,protocol=excluded.protocol,port=excluded.port,
                   baud=excluded.baud,data_bits=excluded.data_bits,stop_bits=excluded.stop_bits,
                   parity=excluded.parity,timeout_ms=excluded.timeout_ms,retry_count=excluded.retry_count,
                   poll_interval_s=excluded.poll_interval_s,config_source='PLATFORM',enabled=excluded.enabled,
                   updated_ms=excluded.updated_ms",
                params![
                    channel.channel_id, channel.channel_name, channel.serial_port,
                    channel.baud_rate, channel.data_bits, channel.stop_bits, parity,
                    channel.timeout_ms, channel.retry_count, channel.poll_interval_s,
                    channel.enabled as i64, now_ms as i64
                ],
            )?;
        }
        for device in devices {
            let has_model = models.iter().any(|model| model.profile == device.profile)
                || tx
                    .query_row(
                        "SELECT COUNT(*) FROM meter_register_map WHERE profile=?1",
                        params![device.profile],
                        |row| row.get::<_, i64>(0),
                    )? > 0;
            if !has_model {
                return Err(
                    StoreError::Validation(
                        format!(
                            "device {} references missing thing model {}", device
                            .device_sn, device.profile
                        ),
                    ),
                );
            }
        }
        for model in models {
            for row in &model.points {
                validate_register_row(row)?;
            }
            for command in &model.commands {
                if command.profile != model.profile
                    || command.command_code.trim().is_empty()
                    || command.function_code != 6
                    || !command.encode_type.eq_ignore_ascii_case("FIXED")
                    || command.fixed_value.is_none()
                {
                    return Err(
                        StoreError::Validation(
                            format!(
                                "model {} contains an unsupported or incomplete protocol command",
                                model.profile
                            ),
                        ),
                    );
                }
            }
        }
        for model in models {
            tx.execute(
                "INSERT INTO thing_model(profile,name,version,source,platform_model_id,enabled,updated_ms)
                 VALUES(?1,?2,?3,'PLATFORM',?4,1,?5)
                 ON CONFLICT(profile) DO UPDATE SET name=excluded.name,version=excluded.version,
                   source='PLATFORM',platform_model_id=excluded.platform_model_id,enabled=1,updated_ms=excluded.updated_ms",
                params![
                    model.profile, model.name, model.version, model.platform_model_id,
                    now_ms as i64
                ],
            )?;
            tx.execute(
                "DELETE FROM meter_register_map WHERE profile=?1",
                params![model.profile],
            )?;
            tx.execute(
                "DELETE FROM protocol_command WHERE profile=?1",
                params![model.profile],
            )?;
            for row in &model.points {
                tx.execute(
                    "INSERT INTO meter_register_map(profile,point_code,point_name,unit,func,address,quantity,field_offset,field_quantity,bit_offset,bit_length,data_type,byte_order,scale,offset)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                    params![
                        row.profile, row.point_code, row.point_name, row.unit, row.func,
                        row.address, row.quantity, row.field_offset, row.field_quantity,
                        row.bit_offset, row.bit_length, row.data_type, row.byte_order,
                        row.scale, row.offset
                    ],
                )?;
            }
            for command in &model.commands {
                tx.execute(
                    "INSERT INTO protocol_command(profile,command_code,command_name,function_code,register_address,encode_type,fixed_value,parameter_json)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        command.profile, command.command_code, command.command_name,
                        command.function_code, command.register_address, command
                        .encode_type, command.fixed_value, command.parameter_json
                    ],
                )?;
            }
        }
        for device in devices {
            let requested_channel = device.channel_id.trim();
            let channel_id = if requested_channel.is_empty() {
                STAGING_CHANNEL_ID.to_string()
            } else {
                requested_channel.to_string()
            };
            let channel_enabled = tx
                .query_row(
                    "SELECT enabled FROM rs485_channel WHERE id=?1",
                    params![channel_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if channel_id == STAGING_CHANNEL_ID || channel_enabled.is_none() {
                return Err(
                    StoreError::Validation(
                        format!(
                            "device {} references missing channel {}", device.device_sn,
                            channel_id
                        ),
                    ),
                );
            }
            if device.enabled && channel_enabled != Some(1) {
                return Err(
                    StoreError::Validation(
                        format!(
                            "device {} references disabled channel {}", device.device_sn,
                            channel_id
                        ),
                    ),
                );
            }
            let can_enable = channel_enabled == Some(1) && device.enabled;
            tx.execute(
                "INSERT INTO meter(device_sn,device_name,modbus_addr,profile,channel_id,config_source,
                   platform_device_id,model_version,upload_enabled,collect_interval_s,enabled,created_ms)
                 VALUES(?1,?2,?3,?4,?5,'PLATFORM',?6,?7,?10,?8,?10,?9)
                 ON CONFLICT(device_sn) DO UPDATE SET device_name=excluded.device_name,modbus_addr=excluded.modbus_addr,
                   profile=excluded.profile,channel_id=excluded.channel_id,config_source='PLATFORM',
                   platform_device_id=excluded.platform_device_id,model_version=excluded.model_version,
                   upload_enabled=?10,
                   collect_interval_s=excluded.collect_interval_s,
                   enabled=?10",
                params![
                    device.device_sn, device.device_name, device.modbus_addr, device
                    .profile, channel_id, device.platform_device_id, device
                    .model_version, device.collect_interval_s as i64, now_ms as i64,
                    can_enable as i64
                ],
            )?;
        }
        let desired_devices = devices
            .iter()
            .map(|device| device.device_sn.as_str())
            .collect::<std::collections::HashSet<_>>();
        let platform_devices = {
            let mut stmt = tx
                .prepare("SELECT device_sn FROM meter WHERE config_source='PLATFORM'")?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for device_sn in platform_devices {
            if !desired_devices.contains(device_sn.as_str()) {
                tx.execute("DELETE FROM meter WHERE device_sn=?1", params![device_sn])?;
            }
        }
        let platform_channels = {
            let mut stmt = tx
                .prepare("SELECT id FROM rs485_channel WHERE config_source='PLATFORM'")?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for channel_id in platform_channels {
            if !channel_ids.contains(&channel_id) {
                tx.execute(
                    "DELETE FROM rs485_channel WHERE id=?1",
                    params![channel_id],
                )?;
            }
        }
        tx.execute(
            "UPDATE config_sync_state SET desired_revision=?1,last_sync_ms=?2,
               last_status='STAGED',last_error=NULL WHERE id=1",
            params![revision, now_ms as i64],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn record_sync_applied(&self, revision: &str, now_ms: u64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE config_sync_state SET applied_revision=?1,last_sync_ms=?2,last_status='APPLIED',last_error=NULL WHERE id=1",
            params![revision, now_ms as i64],
        )?;
        Ok(())
    }
    pub fn record_sync_failure(
        &self,
        revision: Option<&str>,
        error: &str,
        now_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE config_sync_state SET desired_revision=COALESCE(?1,desired_revision),last_sync_ms=?2,
               last_status='FAILED',last_error=?3 WHERE id=1",
            params![revision, now_ms as i64, error],
        )?;
        Ok(())
    }
}
