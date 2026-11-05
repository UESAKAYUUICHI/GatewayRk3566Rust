use super::*;
impl Store {
    /// 返回 true 表示告警从非活动状态转为 ACTIVE，需要产生一次北向事件。
    pub fn upsert_alarm(&self, row: &AlarmEventRecord) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let was_active = conn
            .query_row(
                "SELECT status='ACTIVE' FROM alarm_event_local WHERE source=?1 AND source_event_id=?2",
                params![row.source, row.source_event_id],
                |record| record.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0) == 1;
        conn.execute(
            "INSERT INTO alarm_event_local(source_event_id,source,meter_id,alarm_type,level,point_code,message,status,first_seen_ms,last_seen_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(source,source_event_id) DO UPDATE SET message=excluded.message,level=excluded.level,
               status=excluded.status,last_seen_ms=excluded.last_seen_ms,occurrence_count=occurrence_count+1",
            params![
                row.source_event_id, row.source, row.meter_id, row.alarm_type, row.level,
                row.point_code, row.message, row.status, row.first_seen_ms as i64, row
                .last_seen_ms as i64
            ],
        )?;
        Ok(!was_active && row.status == "ACTIVE")
    }
    pub fn resolve_alarm(
        &self,
        source: &str,
        source_event_id: &str,
        now_ms: u64,
    ) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn
            .execute(
                "UPDATE alarm_event_local SET status='RECOVERED',last_seen_ms=?3
             WHERE source=?1 AND source_event_id=?2 AND status='ACTIVE'",
                params![source, source_event_id, now_ms as i64],
            )?;
        Ok(changed > 0)
    }
    pub fn mark_alarm_cloud_pending(
        &self,
        source_event_id: &str,
        message_id: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE alarm_event_local SET cloud_message_id=?2,cloud_status='QUEUED',cloud_ack_ms=NULL
             WHERE source='GATEWAY' AND source_event_id=?1",
            params![source_event_id, message_id],
        )?;
        Ok(())
    }
    pub fn mark_alarm_cloud_ack(
        &self,
        source_event_id: &str,
        status: &str,
        ack_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE alarm_event_local SET cloud_status=?2,cloud_ack_ms=?3
             WHERE source='GATEWAY' AND source_event_id=?1",
            params![source_event_id, status, ack_ms as i64],
        )?;
        Ok(())
    }
    /// 平台下行只返回当前活动告警：本轮未返回的平台告警在网关侧标记为已恢复。
    pub fn replace_platform_alarms(
        &self,
        alarms: &[AlarmEventRecord],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE alarm_event_local SET status='RECOVERED',last_seen_ms=?1 WHERE source='PLATFORM' AND status='ACTIVE'",
            params![now_ms as i64],
        )?;
        for row in alarms {
            tx.execute(
                "INSERT INTO alarm_event_local(source_event_id,source,meter_id,alarm_type,level,point_code,message,status,first_seen_ms,last_seen_ms,cloud_status)
                 VALUES(?1,'PLATFORM',?2,?3,?4,?5,?6,'ACTIVE',?7,?8,'SYNCED')
                 ON CONFLICT(source,source_event_id) DO UPDATE SET meter_id=excluded.meter_id,
                   alarm_type=excluded.alarm_type,level=excluded.level,point_code=excluded.point_code,
                   message=excluded.message,status='ACTIVE',last_seen_ms=excluded.last_seen_ms,cloud_status='SYNCED'",
                params![
                    row.source_event_id, row.meter_id, row.alarm_type, row.level, row
                    .point_code, row.message, row.first_seen_ms as i64, row.last_seen_ms
                    as i64
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn recent_alarms(&self, limit: u32) -> StoreResult<Vec<AlarmEventRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT source_event_id,source,meter_id,alarm_type,level,point_code,message,status,
                    first_seen_ms,last_seen_ms,cloud_status,cloud_message_id,cloud_ack_ms
             FROM alarm_event_local ORDER BY last_seen_ms DESC LIMIT ?1",
            )?;
        Ok(
            stmt
                .query_map(
                    params![limit],
                    |row| {
                        Ok(AlarmEventRecord {
                            source_event_id: row.get(0)?,
                            source: row.get(1)?,
                            meter_id: row.get(2)?,
                            alarm_type: row.get(3)?,
                            level: row.get(4)?,
                            point_code: row.get(5)?,
                            message: row.get(6)?,
                            status: row.get(7)?,
                            first_seen_ms: row.get::<_, i64>(8)? as u64,
                            last_seen_ms: row.get::<_, i64>(9)? as u64,
                            cloud_status: row.get(10)?,
                            cloud_message_id: row.get(11)?,
                            cloud_ack_ms: row
                                .get::<_, Option<i64>>(12)?
                                .map(|value| value as u64),
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub fn acknowledge_active_alarms(&self, ack_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn
            .execute(
                "UPDATE alarm_event_local SET status='ACKED', last_seen_ms=?1 WHERE status='ACTIVE'",
                params![ack_ms as i64],
            )?;
        Ok(changed)
    }
}
