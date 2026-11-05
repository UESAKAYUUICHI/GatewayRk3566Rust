use super::*;
impl Store {
    pub fn alarm_rules(&self) -> StoreResult<Vec<AlarmRuleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id,rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms
             FROM alarm_rule ORDER BY source DESC, id DESC",
            )?;
        Ok(
            stmt
                .query_map(
                    [],
                    |row| {
                        Ok(AlarmRuleRecord {
                            id: row.get(0)?,
                            rule_code: row.get(1)?,
                            source: row.get(2)?,
                            name: row.get(3)?,
                            level: row.get(4)?,
                            target_device_sn: row.get(5)?,
                            point_code: row.get(6)?,
                            operator: row.get(7)?,
                            threshold: row.get(8)?,
                            unit: row.get(9)?,
                            duration_s: row.get::<_, i64>(10)? as u64,
                            enabled: row.get::<_, i64>(11)? == 1,
                            updated_ms: row.get::<_, i64>(12)? as u64,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub fn upsert_local_alarm_rule(
        &self,
        id: Option<i64>,
        row: &AlarmRuleRecord,
    ) -> StoreResult<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        if let Some(id) = id {
            let source = conn
                .query_row(
                    "SELECT source FROM alarm_rule WHERE id=?1",
                    params![id],
                    |record| record.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| StoreError::Validation("alarm rule not found".into()))?;
            if source != "LOCAL" {
                return Err(
                    StoreError::Validation("platform alarm rule is locked".into()),
                );
            }
            conn.execute(
                "UPDATE alarm_rule SET rule_code=?2,name=?3,level=?4,target_device_sn=?5,point_code=?6,
                    operator=?7,threshold=?8,unit=?9,duration_s=?10,enabled=?11,updated_ms=?12
                 WHERE id=?1 AND source='LOCAL'",
                params![
                    id, row.rule_code, row.name, row.level, row.target_device_sn, row
                    .point_code, row.operator, row.threshold, row.unit, row.duration_s as
                    i64, row.enabled as i64, row.updated_ms as i64
                ],
            )?;
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO alarm_rule(rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms)
             VALUES(?1,'LOCAL',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                row.rule_code, row.name, row.level, row.target_device_sn, row.point_code,
                row.operator, row.threshold, row.unit, row.duration_s as i64, row.enabled
                as i64, row.updated_ms as i64
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
    pub fn delete_local_alarm_rule(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn
            .execute(
                "DELETE FROM alarm_rule WHERE id=?1 AND source='LOCAL'",
                params![id],
            )?;
        if changed == 0 {
            return Err(
                StoreError::Validation(
                    "alarm rule not found or platform rule is locked".into(),
                ),
            );
        }
        Ok(())
    }
    pub fn replace_platform_alarm_rules(
        &self,
        rules: &[AlarmRuleRecord],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM alarm_rule WHERE source='PLATFORM'", [])?;
        for row in rules {
            tx.execute(
                "INSERT INTO alarm_rule(rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms)
                 VALUES(?1,'PLATFORM',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    row.rule_code, row.name, row.level, row.target_device_sn, row
                    .point_code, row.operator, row.threshold, row.unit, row.duration_s as
                    i64, row.enabled as i64, now_ms as i64
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
