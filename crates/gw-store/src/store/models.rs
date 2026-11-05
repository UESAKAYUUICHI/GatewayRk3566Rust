use super::*;
impl Store {
    pub fn thing_models(&self) -> StoreResult<Vec<ThingModelSummary>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT p.profile,COALESCE(t.name,p.profile),COALESCE(t.version,'local-v1'),
                    COALESCE(t.source,'LOCAL'),COUNT(DISTINCT p.point_code),COUNT(DISTINCT m.id)
             FROM meter_register_map p LEFT JOIN thing_model t ON t.profile=p.profile
             LEFT JOIN meter m ON m.profile=p.profile GROUP BY p.profile,t.name,t.version,t.source ORDER BY p.profile",
            )?;
        Ok(
            stmt
                .query_map(
                    [],
                    |r| {
                        Ok(ThingModelSummary {
                            profile: r.get(0)?,
                            name: r.get(1)?,
                            version: r.get(2)?,
                            source: r.get(3)?,
                            point_count: r.get::<_, i64>(4)? as u32,
                            device_count: r.get::<_, i64>(5)? as u32,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub fn delete_thing_model(&self, profile: &str) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let used: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meter WHERE profile=?1",
                params![profile],
                |r| r.get(0),
            )?;
        if used > 0 {
            return Err(
                StoreError::Validation(
                    format!("thing model {profile} is used by {used} device(s)"),
                ),
            );
        }
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meter_register_map WHERE profile=?1", params![profile])?;
        tx.execute("DELETE FROM thing_model WHERE profile=?1", params![profile])?;
        tx.commit()?;
        Ok(())
    }
    pub fn register_local_thing_model(
        &self,
        profile: &str,
        name: &str,
        now_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO thing_model(profile,name,version,source,enabled,updated_ms)
             VALUES(?1,?2,'local-v1','LOCAL',1,?3)
             ON CONFLICT(profile) DO NOTHING",
            params![profile, name, now_ms as i64],
        )?;
        Ok(())
    }
    /// 以覆盖方式导入一个档案（事务内先删该 profile 再插入全部行）。
    pub fn replace_register_map(
        &self,
        profile: &str,
        rows: &[RegisterMapRow],
    ) -> StoreResult<()> {
        for row in rows {
            validate_register_row(row)?;
        }
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meter_register_map WHERE profile=?1", params![profile])?;
        for row in rows {
            tx.execute(
                "INSERT INTO meter_register_map(profile, point_code, point_name, unit, func, address, quantity, field_offset, field_quantity, bit_offset, bit_length, data_type, byte_order, scale, offset)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    row.profile, row.point_code, row.point_name, row.unit, row.func, row
                    .address, row.quantity, row.field_offset, row.field_quantity, row
                    .bit_offset, row.bit_length, row.data_type, row.byte_order, row
                    .scale, row.offset
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn register_map(&self, profile: &str) -> StoreResult<Vec<RegisterMapRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT profile, point_code, point_name, unit, func, address, quantity, field_offset, field_quantity, bit_offset, bit_length, data_type, byte_order, scale, offset
             FROM meter_register_map WHERE profile=?1 ORDER BY address",
            )?;
        let rows = stmt
            .query_map(
                params![profile],
                |r| {
                    Ok(RegisterMapRow {
                        profile: r.get(0)?,
                        point_code: r.get(1)?,
                        point_name: r.get(2)?,
                        unit: r.get(3)?,
                        func: r.get::<_, i64>(4)? as u8,
                        address: r.get::<_, i64>(5)? as u16,
                        quantity: r.get::<_, i64>(6)? as u16,
                        field_offset: r.get::<_, i64>(7)? as u16,
                        field_quantity: r.get::<_, i64>(8)? as u16,
                        bit_offset: r.get::<_, Option<i64>>(9)?.map(|v| v as u8),
                        bit_length: r.get::<_, Option<i64>>(10)?.map(|v| v as u8),
                        data_type: r.get(11)?,
                        byte_order: r.get(12)?,
                        scale: r.get(13)?,
                        offset: r.get(14)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn protocol_command(
        &self,
        profile: &str,
        command_code: &str,
    ) -> StoreResult<Option<ProtocolCommandRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.query_row(
                "SELECT profile,command_code,command_name,function_code,register_address,encode_type,fixed_value,parameter_json
             FROM protocol_command WHERE profile=?1 AND command_code=?2",
                params![profile, command_code],
                |r| Ok(ProtocolCommandRow {
                    profile: r.get(0)?,
                    command_code: r.get(1)?,
                    command_name: r.get(2)?,
                    function_code: r.get::<_, i64>(3)? as u8,
                    register_address: r.get::<_, i64>(4)? as u16,
                    encode_type: r.get(5)?,
                    fixed_value: r.get::<_, Option<i64>>(6)?.map(|value| value as u16),
                    parameter_json: r.get(7)?,
                }),
            )
            .optional()
            .map_err(StoreError::from)
    }
}
