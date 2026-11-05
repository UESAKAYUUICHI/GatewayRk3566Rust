use super::*;
impl Store {
    pub fn insert_meter(&self, input: &MeterInput, now_ms: u64) -> StoreResult<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO meter(device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                               model_version, upload_enabled, collect_interval_s, enabled, created_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, 'LOCAL', 'local-v1', ?6, ?7, ?8, ?9)",
            params![
                input.device_sn, input.device_name, input.modbus_addr, input.profile,
                input.channel_id, input.upload_enabled as i64, input.collect_interval_s
                as i64, input.enabled as i64, now_ms as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
    pub fn update_meter(&self, id: i64, input: &MeterInput) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn
            .execute(
                "UPDATE meter SET device_sn=?2, device_name=?3, modbus_addr=?4, profile=?5, channel_id=?6,
                              upload_enabled=?7, collect_interval_s=?8, enabled=?9
             WHERE id=?1",
                params![
                    id, input.device_sn, input.device_name, input.modbus_addr, input
                    .profile, input.channel_id, input.upload_enabled as i64, input
                    .collect_interval_s as i64, input.enabled as i64
                ],
            )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }
    pub fn delete_meter(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("DELETE FROM meter WHERE id=?1", params![id])?;
        Ok(())
    }
    pub fn meters(&self) -> StoreResult<Vec<MeterRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                    platform_device_id, model_version, upload_enabled, collect_interval_s, enabled, created_ms
             FROM meter ORDER BY id",
            )?;
        let rows = stmt
            .query_map(
                [],
                |r| {
                    Ok(MeterRecord {
                        id: r.get(0)?,
                        device_sn: r.get(1)?,
                        device_name: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        profile: r.get(4)?,
                        channel_id: r.get(5)?,
                        config_source: r.get(6)?,
                        platform_device_id: r.get(7)?,
                        model_version: r.get(8)?,
                        upload_enabled: r.get::<_, i64>(9)? == 1,
                        collect_interval_s: r.get::<_, i64>(10)? as u64,
                        enabled: r.get::<_, i64>(11)? == 1,
                        created_ms: r.get::<_, i64>(12)? as u64,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn meter_by_sn(&self, device_sn: &str) -> StoreResult<Option<MeterRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let record = conn
            .query_row(
                "SELECT id, device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                        platform_device_id, model_version, upload_enabled, collect_interval_s, enabled, created_ms
                 FROM meter WHERE device_sn=?1",
                params![device_sn],
                |r| {
                    Ok(MeterRecord {
                        id: r.get(0)?,
                        device_sn: r.get(1)?,
                        device_name: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        profile: r.get(4)?,
                        channel_id: r.get(5)?,
                        config_source: r.get(6)?,
                        platform_device_id: r.get(7)?,
                        model_version: r.get(8)?,
                        upload_enabled: r.get::<_, i64>(9)? == 1,
                        collect_interval_s: r.get::<_, i64>(10)? as u64,
                        enabled: r.get::<_, i64>(11)? == 1,
                        created_ms: r.get::<_, i64>(12)? as u64,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }
}
