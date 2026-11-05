use super::*;
impl Store {
    /// 入队。message_id 冲突（进程内重试/重复入队）静默忽略，返回是否真正插入。
    pub fn enqueue(&self, item: &NewOutbox) -> StoreResult<bool> {
        self.enqueue_with_status(item, "pending")
    }
    /// 以指定状态入队（'pending' 或 'deferred'：时钟不可信期间暂存、待补时间戳的行）。
    pub fn enqueue_with_status(
        &self,
        item: &NewOutbox,
        status: &str,
    ) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO outbox_message(message_id, topic, payload, created_ms, status)
             VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    item.message_id, item.topic, item.payload, item.created_ms as i64,
                    status
                ],
            )?;
        Ok(changed == 1)
    }
    /// 原子地创建一个上报批次并标记其包含的历史样本。进程在任意一步退出都不会丢样本。
    pub fn enqueue_sample_batch(
        &self,
        item: &NewOutbox,
        status: &str,
        sample_ids: &[i64],
    ) -> StoreResult<bool> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let changed = tx
            .execute(
                "INSERT OR IGNORE INTO outbox_message(message_id,topic,payload,created_ms,status)
             VALUES(?1,?2,?3,?4,?5)",
                params![
                    item.message_id, item.topic, item.payload, item.created_ms as i64,
                    status
                ],
            )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(false);
        }
        let outbox_id = tx.last_insert_rowid();
        for sample_id in sample_ids {
            tx.execute(
                "UPDATE meter_sample SET outbox_id=?2 WHERE id=?1 AND outbox_id IS NULL",
                params![sample_id, outbox_id],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }
    /// 取延迟暂存行（按入队顺序）。
    pub fn deferred_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, topic, payload, created_ms, attempts
             FROM outbox_message WHERE status='deferred' ORDER BY id LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(OutboxItem {
                        id: r.get(0)?,
                        message_id: r.get(1)?,
                        topic: r.get(2)?,
                        payload: r.get(3)?,
                        created_ms: r.get::<_, i64>(4)? as u64,
                        attempts: r.get::<_, u32>(5)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    /// 把延迟行定稿为待发布行（补时间戳与正式 messageId）。
    pub fn finalize_outbox(
        &self,
        id: i64,
        message_id: &str,
        payload: &[u8],
        created_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET message_id=?2, payload=?3, created_ms=?4, status='pending'
             WHERE id=?1",
            params![id, message_id, payload, created_ms as i64],
        )?;
        Ok(())
    }
    /// 按入队顺序取待发布报文。
    pub fn pending_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, topic, payload, created_ms, attempts
             FROM outbox_message WHERE status='pending' ORDER BY id LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(OutboxItem {
                        id: r.get(0)?,
                        message_id: r.get(1)?,
                        topic: r.get(2)?,
                        payload: r.get(3)?,
                        created_ms: r.get::<_, i64>(4)? as u64,
                        attempts: r.get::<_, u32>(5)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn recent_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, topic, payload, created_ms, status, attempts, last_error
             FROM outbox_message ORDER BY created_ms DESC, id DESC LIMIT ?1",
            )?;
        let rows = stmt
            .query_map(
                params![limit],
                |r| {
                    Ok(OutboxRecord {
                        id: r.get(0)?,
                        message_id: r.get(1)?,
                        topic: r.get(2)?,
                        payload: r.get(3)?,
                        created_ms: r.get::<_, i64>(4)? as u64,
                        status: r.get(5)?,
                        attempts: r.get::<_, u32>(6)?,
                        last_error: r.get(7)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn quarantine_outbox(&self, id: i64, error: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET status='quarantined', attempts=attempts+1, last_error=?2 WHERE id=?1 AND status<>'sent'",
            params![id, error],
        )?;
        Ok(())
    }
    pub fn mark_sent(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET status='sent', last_error=NULL WHERE id=?1 AND status='pending'",
            params![id],
        )?;
        Ok(())
    }
    /// 记录一次失败尝试（保持 pending，供补偿重试），返回累计尝试次数。
    pub fn mark_attempt_failed(&self, id: i64, error: &str) -> StoreResult<u32> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET attempts=attempts+1, last_error=?2 WHERE id=?1",
            params![id, error],
        )?;
        let attempts: u32 = conn
            .query_row(
                "SELECT attempts FROM outbox_message WHERE id=?1",
                params![id],
                |r| r.get(0),
            )?;
        Ok(attempts)
    }
    pub fn outbox_pending_count(&self) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox_message WHERE status='pending'",
                [],
                |r| r.get(0),
            )?;
        Ok(count as u64)
    }
    pub fn outbox_sent_count(&self) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox_message WHERE status='sent'",
                [],
                |r| r.get(0),
            )?;
        Ok(count as u64)
    }
    /// 清理 cutoff 之前的已发布行（防无限增长）。
    pub fn cleanup_sent_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let deleted = conn
            .execute(
                "DELETE FROM outbox_message WHERE status='sent' AND created_ms < ?1",
                params![cutoff_ms as i64],
            )?;
        Ok(deleted)
    }
}
