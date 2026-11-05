use super::*;
impl Store {
    pub fn channels(&self) -> StoreResult<Vec<Rs485ChannelRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id,name,protocol,port,baud,data_bits,stop_bits,parity,timeout_ms,retry_count,
                    poll_interval_s,config_source,enabled
             FROM rs485_channel ORDER BY id",
            )?;
        Ok(
            stmt
                .query_map(
                    [],
                    |r| {
                        Ok(Rs485ChannelRecord {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            protocol: r.get(2)?,
                            port: r.get(3)?,
                            baud: r.get::<_, i64>(4)? as u32,
                            data_bits: r.get::<_, i64>(5)? as u8,
                            stop_bits: r.get::<_, i64>(6)? as u8,
                            parity: r.get(7)?,
                            timeout_ms: r.get::<_, i64>(8)? as u64,
                            retry_count: r.get::<_, i64>(9)? as u32,
                            poll_interval_s: r.get::<_, i64>(10)? as u64,
                            config_source: r.get(11)?,
                            enabled: r.get::<_, i64>(12)? == 1,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub fn channel_enabled(&self, channel_id: &str) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(
            conn
                .query_row(
                    "SELECT enabled FROM rs485_channel WHERE id=?1",
                    params![channel_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .map(|value| value == 1)
                .unwrap_or(false),
        )
    }
    pub fn upsert_channel(
        &self,
        row: &Rs485ChannelRecord,
        now_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO rs485_channel(id,name,protocol,port,baud,data_bits,stop_bits,parity,timeout_ms,
                retry_count,poll_interval_s,config_source,enabled,updated_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,protocol=excluded.protocol,port=excluded.port,
               baud=excluded.baud,data_bits=excluded.data_bits,stop_bits=excluded.stop_bits,
               parity=excluded.parity,timeout_ms=excluded.timeout_ms,retry_count=excluded.retry_count,
               poll_interval_s=excluded.poll_interval_s,config_source=excluded.config_source,
               enabled=excluded.enabled,updated_ms=excluded.updated_ms",
            params![
                row.id, row.name, row.protocol, row.port, row.baud, row.data_bits, row
                .stop_bits, row.parity, row.timeout_ms, row.retry_count, row
                .poll_interval_s, row.config_source, row.enabled as i64, now_ms as i64
            ],
        )?;
        Ok(())
    }
    pub fn delete_channel_with_meters(&self, channel_id: &str) -> StoreResult<usize> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM rs485_channel WHERE id=?1",
                params![channel_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(
                StoreError::Validation(format!("missing RS485 channel {channel_id}")),
            );
        }
        let deleted_meters = tx
            .execute("DELETE FROM meter WHERE channel_id=?1", params![channel_id])?;
        tx.execute("DELETE FROM rs485_channel WHERE id=?1", params![channel_id])?;
        tx.commit()?;
        Ok(deleted_meters)
    }
    pub fn bind_meter_to_channel(
        &self,
        meter_id: i64,
        channel_id: &str,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let channel_enabled: Option<i64> = tx
            .query_row(
                "SELECT enabled FROM rs485_channel WHERE id=?1",
                params![channel_id],
                |row| row.get(0),
            )
            .optional()?;
        if channel_enabled.is_none() {
            return Err(
                StoreError::Validation(format!("missing RS485 channel {channel_id}")),
            );
        }
        if channel_enabled != Some(1) {
            return Err(
                StoreError::Validation(format!("disabled RS485 channel {channel_id}")),
            );
        }
        let source: String = tx
            .query_row(
                "SELECT config_source FROM meter WHERE id=?1",
                params![meter_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Validation("device not found".into()))?;
        let modbus_addr: i64 = tx
            .query_row(
                "SELECT modbus_addr FROM meter WHERE id=?1",
                params![meter_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Validation("device not found".into()))?;
        let duplicated: Option<String> = tx
            .query_row(
                "SELECT device_sn FROM meter WHERE channel_id=?1 AND modbus_addr=?2 AND id<>?3",
                params![channel_id, modbus_addr, meter_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(device_sn) = duplicated {
            return Err(
                StoreError::Validation(
                    format!(
                        "RS485 channel {channel_id} already has Modbus address {modbus_addr} on {device_sn}"
                    ),
                ),
            );
        }
        tx.execute(
            "UPDATE meter SET channel_id=?2,enabled=1,
               upload_enabled=CASE WHEN ?3='PLATFORM' THEN 1 ELSE upload_enabled END
             WHERE id=?1",
            params![meter_id, channel_id, source],
        )?;
        tx.commit()?;
        Ok(())
    }
}
