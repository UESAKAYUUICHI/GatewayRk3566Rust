use super::*;
impl Store {
    pub fn save_reading(
        &self,
        record: &ReadingRecord,
        updated_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO meter_reading(meter_id, snapshot, read_ms, quality, updated_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(meter_id) DO UPDATE SET
                snapshot=excluded.snapshot, read_ms=excluded.read_ms,
                quality=excluded.quality, updated_ms=excluded.updated_ms",
            params![
                record.meter_id, record.snapshot_json, record.read_ms as i64, record
                .quality, updated_ms as i64
            ],
        )?;
        Ok(())
    }
    /// 保存最新快照的同时追加不可覆盖的历史样本。
    pub fn save_sample(
        &self,
        record: &ReadingRecord,
        updated_ms: u64,
    ) -> StoreResult<i64> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meter_reading(meter_id,snapshot,read_ms,quality,updated_ms)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(meter_id) DO UPDATE SET snapshot=excluded.snapshot,read_ms=excluded.read_ms,
               quality=excluded.quality,updated_ms=excluded.updated_ms",
            params![
                record.meter_id, record.snapshot_json, record.read_ms as i64, record
                .quality, updated_ms as i64
            ],
        )?;
        let inserted = tx.execute(
            "INSERT INTO meter_sample(meter_id,read_ms,sample_interval_s,quality,points_json,created_ms,metadata_json)
             SELECT ?1,?2,?3,?4,?5,?6,json_object(
               'device_sn',m.device_sn,'modbus_addr',m.modbus_addr,'channel_id',m.channel_id,
               'profile',m.profile,'model_version',m.model_version,
               'config_revision',(SELECT applied_revision FROM config_sync_state WHERE id=1))
             FROM meter m WHERE m.id=?1",
            params![
                record.meter_id, record.read_ms as i64, record.sample_interval_s as i64,
                record.quality, record.snapshot_json, updated_ms as i64
            ],
        )?;
        if inserted != 1 {
            return Err(StoreError::Validation("sample meter does not exist".into()));
        }
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }
    pub fn unbatched_samples(&self, limit: u32) -> StoreResult<Vec<SampleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT s.id,s.meter_id,COALESCE(json_extract(s.metadata_json,'$.device_sn'),m.device_sn),
                    COALESCE(json_extract(s.metadata_json,'$.modbus_addr'),m.modbus_addr),
                    COALESCE(json_extract(s.metadata_json,'$.channel_id'),m.channel_id),
                    COALESCE(json_extract(s.metadata_json,'$.profile'),m.profile),
                    COALESCE(json_extract(s.metadata_json,'$.model_version'),m.model_version),
                    CASE WHEN s.metadata_json IS NULL THEN (SELECT applied_revision FROM config_sync_state WHERE id=1)
                      ELSE json_extract(s.metadata_json,'$.config_revision') END,
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             WHERE s.outbox_id IS NULL AND m.upload_enabled=1
             ORDER BY s.id LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(SampleRecord {
                        id: r.get(0)?,
                        meter_id: r.get(1)?,
                        device_sn: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        channel_id: r.get(4)?,
                        profile: r.get(5)?,
                        model_version: r.get(6)?,
                        config_revision: r.get(7)?,
                        read_ms: r.get::<_, i64>(8)? as u64,
                        sample_interval_s: r.get::<_, i64>(9)? as u64,
                        quality: r.get::<_, i64>(10)? as u32,
                        points_json: r.get(11)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn sample_history_since(
        &self,
        from_ms: u64,
        limit: u32,
    ) -> StoreResult<Vec<SampleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT s.id,s.meter_id,COALESCE(json_extract(s.metadata_json,'$.device_sn'),m.device_sn),
                    COALESCE(json_extract(s.metadata_json,'$.modbus_addr'),m.modbus_addr),
                    COALESCE(json_extract(s.metadata_json,'$.channel_id'),m.channel_id),
                    COALESCE(json_extract(s.metadata_json,'$.profile'),m.profile),
                    COALESCE(json_extract(s.metadata_json,'$.model_version'),m.model_version),
                    CASE WHEN s.metadata_json IS NULL THEN (SELECT applied_revision FROM config_sync_state WHERE id=1)
                      ELSE json_extract(s.metadata_json,'$.config_revision') END,
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             WHERE s.read_ms>=?1 ORDER BY s.read_ms,s.id LIMIT ?2",
            )?;
        let rows = stmt
            .query_map(
                params![from_ms as i64, limit],
                |r| {
                    Ok(SampleRecord {
                        id: r.get(0)?,
                        meter_id: r.get(1)?,
                        device_sn: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        channel_id: r.get(4)?,
                        profile: r.get(5)?,
                        model_version: r.get(6)?,
                        config_revision: r.get(7)?,
                        read_ms: r.get::<_, i64>(8)? as u64,
                        sample_interval_s: r.get::<_, i64>(9)? as u64,
                        quality: r.get::<_, i64>(10)? as u32,
                        points_json: r.get(11)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn recent_samples_page(
        &self,
        page_num: u32,
        page_size: u32,
    ) -> StoreResult<(Vec<SampleRecord>, u64)> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM meter_sample", [], |row| row.get(0))?;
        let page_num = page_num.max(1);
        let page_size = page_size.clamp(1, 500);
        let offset = (page_num - 1) * page_size;
        let mut stmt = conn
            .prepare(
                "SELECT s.id,s.meter_id,COALESCE(json_extract(s.metadata_json,'$.device_sn'),m.device_sn),
                    COALESCE(json_extract(s.metadata_json,'$.modbus_addr'),m.modbus_addr),
                    COALESCE(json_extract(s.metadata_json,'$.channel_id'),m.channel_id),
                    COALESCE(json_extract(s.metadata_json,'$.profile'),m.profile),
                    COALESCE(json_extract(s.metadata_json,'$.model_version'),m.model_version),
                    CASE WHEN s.metadata_json IS NULL THEN (SELECT applied_revision FROM config_sync_state WHERE id=1)
                      ELSE json_extract(s.metadata_json,'$.config_revision') END,
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             ORDER BY s.read_ms DESC,s.id DESC LIMIT ?1 OFFSET ?2",
            )?;
        let rows = stmt
            .query_map(
                params![page_size, offset],
                |r| {
                    Ok(SampleRecord {
                        id: r.get(0)?,
                        meter_id: r.get(1)?,
                        device_sn: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        channel_id: r.get(4)?,
                        profile: r.get(5)?,
                        model_version: r.get(6)?,
                        config_revision: r.get(7)?,
                        read_ms: r.get::<_, i64>(8)? as u64,
                        sample_interval_s: r.get::<_, i64>(9)? as u64,
                        quality: r.get::<_, i64>(10)? as u32,
                        points_json: r.get(11)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, total as u64))
    }
    pub fn cleanup_samples_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(
            conn
                .execute(
                    "DELETE FROM meter_sample WHERE read_ms<?1 AND outbox_id IN
             (SELECT id FROM outbox_message WHERE status='sent')",
                    params![cutoff_ms as i64],
                )?,
        )
    }
    /// LOCAL 设备默认不进入正式上报，但仍保留 7 天本地分析数据。
    pub fn cleanup_local_samples_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(
            conn
                .execute(
                    "DELETE FROM meter_sample WHERE read_ms<?1 AND outbox_id IS NULL AND meter_id IN
             (SELECT id FROM meter WHERE upload_enabled=0)",
                    params![cutoff_ms as i64],
                )?,
        )
    }
    pub fn reading(&self, meter_id: i64) -> StoreResult<Option<ReadingRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let record = conn
            .query_row(
                "SELECT meter_id, snapshot, read_ms, quality FROM meter_reading WHERE meter_id=?1",
                params![meter_id],
                |r| {
                    Ok(ReadingRecord {
                        meter_id: r.get(0)?,
                        snapshot_json: r.get(1)?,
                        read_ms: r.get::<_, i64>(2)? as u64,
                        sample_interval_s: 0,
                        quality: r.get::<_, i64>(3)? as u32,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }
}
