use super::*;
impl Store {
    pub fn record_command(&self, row: &CommandLogRow) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO command_log(command_id, command_type, target_sn, payload, received_ms, result_status, responded_ms, message)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(command_id) DO UPDATE SET
                result_status=excluded.result_status,
                responded_ms=excluded.responded_ms,
                message=excluded.message",
            params![
                row.command_id, row.command_type, row.target_sn, row.payload_json, row
                .received_ms as i64, row.result_status, row.responded_ms as i64, row
                .message
            ],
        )?;
        Ok(())
    }
    pub fn command_result(
        &self,
        command_id: &str,
    ) -> StoreResult<Option<(String, String)>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.query_row(
                "SELECT result_status,message FROM command_log WHERE command_id=?1 AND result_status IS NOT NULL",
                params![command_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(StoreError::from)
    }
    pub fn recent_commands(&self, limit: u32) -> StoreResult<Vec<CommandLogRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT command_id, command_type, target_sn, payload, received_ms, result_status, responded_ms, message
             FROM command_log ORDER BY id DESC LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(CommandLogRow {
                        command_id: r.get(0)?,
                        command_type: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        target_sn: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        payload_json: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        received_ms: r.get::<_, i64>(4)? as u64,
                        result_status: r
                            .get::<_, Option<String>>(5)?
                            .unwrap_or_default(),
                        responded_ms: r.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                        message: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn log_event(&self, row: &EventRow) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO event_log(ts_ms, level, source, message) VALUES(?1, ?2, ?3, ?4)",
            params![row.ts_ms as i64, row.level, row.source, row.message],
        )?;
        Ok(())
    }
    pub fn recent_events(&self, limit: u32) -> StoreResult<Vec<EventRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT ts_ms, level, source, message FROM event_log ORDER BY id DESC LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(EventRow {
                        ts_ms: r.get::<_, i64>(0)? as u64,
                        level: r.get(1)?,
                        source: r.get(2)?,
                        message: r.get(3)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    /// 事件日志裁剪：只保留最近 limit 行。
    pub fn trim_events(&self, limit: u32) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let deleted = conn
            .execute(
                "DELETE FROM event_log WHERE id <= (SELECT id FROM event_log ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                params![limit],
            )?;
        Ok(deleted)
    }
}
