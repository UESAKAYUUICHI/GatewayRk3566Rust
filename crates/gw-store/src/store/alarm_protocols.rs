use super::*;
impl Store {
    pub fn replace_platform_alarm_protocols(
        &self,
        protocols: &[AlarmProtocolRecord],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM alarm_protocol_binding", [])?;
        tx.execute("DELETE FROM alarm_protocol_point", [])?;
        tx.execute("DELETE FROM alarm_protocol", [])?;
        for protocol in protocols {
            tx.execute(
                "INSERT INTO alarm_protocol(protocol_id,protocol_key,protocol_name,version_no,enabled,updated_ms)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    protocol.protocol_id, protocol.protocol_key, protocol.protocol_name,
                    protocol.version_no, protocol.enabled as i64, now_ms as i64
                ],
            )?;
            for point in &protocol.points {
                tx.execute(
                    "INSERT INTO alarm_protocol_point
                       (point_id,protocol_id,point_code,point_name,alarm_type,level,operator,
                        threshold_value,threshold_min,threshold_max,recovery_threshold_value,duration_s,enabled)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                    params![
                        point.point_id, protocol.protocol_id, point.point_code, point
                        .point_name, point.alarm_type, point.level, point.operator, point
                        .threshold_value, point.threshold_min, point.threshold_max, point
                        .recovery_threshold_value, point.duration_s as i64, point.enabled
                        as i64
                    ],
                )?;
            }
            for device in &protocol.devices {
                tx.execute(
                    "INSERT INTO alarm_protocol_binding(protocol_id,device_sn,enabled) VALUES(?1,?2,1)",
                    params![protocol.protocol_id, device.device_sn],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn alarm_protocol_points_for_meter(
        &self,
        meter_id: i64,
    ) -> StoreResult<Vec<AlarmProtocolPointRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT pp.point_id,pp.protocol_id,pp.point_code,pp.point_name,pp.alarm_type,pp.level,
                    pp.operator,pp.threshold_value,pp.threshold_min,pp.threshold_max,
                    pp.recovery_threshold_value,pp.duration_s,pp.enabled
             FROM meter m
             JOIN alarm_protocol_binding b ON b.device_sn=m.device_sn AND b.enabled=1
             JOIN alarm_protocol p ON p.protocol_id=b.protocol_id AND p.enabled=1
             JOIN alarm_protocol_point pp ON pp.protocol_id=p.protocol_id AND pp.enabled=1
             WHERE m.id=?1
             ORDER BY pp.protocol_id,pp.point_id",
            )?;
        Ok(
            stmt
                .query_map(
                    params![meter_id],
                    |row| {
                        Ok(AlarmProtocolPointRecord {
                            point_id: row.get(0)?,
                            protocol_id: row.get(1)?,
                            point_code: row.get(2)?,
                            point_name: row.get(3)?,
                            alarm_type: row.get(4)?,
                            level: row.get(5)?,
                            operator: row.get(6)?,
                            threshold_value: row.get(7)?,
                            threshold_min: row.get(8)?,
                            threshold_max: row.get(9)?,
                            recovery_threshold_value: row.get(10)?,
                            duration_s: row.get::<_, i64>(11)? as u64,
                            enabled: row.get::<_, i64>(12)? == 1,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub fn alarm_protocols(&self) -> StoreResult<Vec<AlarmProtocolRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT protocol_id,protocol_key,protocol_name,version_no,enabled,updated_ms
             FROM alarm_protocol ORDER BY protocol_id",
            )?;
        let mut result = Vec::new();
        for row in stmt
            .query_map(
                [],
                |row| {
                    Ok(AlarmProtocolRecord {
                        protocol_id: row.get(0)?,
                        protocol_key: row.get(1)?,
                        protocol_name: row.get(2)?,
                        version_no: row.get(3)?,
                        enabled: row.get::<_, i64>(4)? == 1,
                        updated_ms: row.get::<_, i64>(5)? as u64,
                        devices: Vec::new(),
                        points: Vec::new(),
                    })
                },
            )?
        {
            let mut protocol = row?;
            let mut device_stmt = conn
                .prepare(
                    "SELECT device_sn FROM alarm_protocol_binding WHERE protocol_id=?1 AND enabled=1",
                )?;
            protocol.devices = device_stmt
                .query_map(
                    params![protocol.protocol_id],
                    |item| {
                        Ok(AlarmProtocolDeviceRecord {
                            device_sn: item.get(0)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let mut point_stmt = conn
                .prepare(
                    "SELECT point_id,protocol_id,point_code,point_name,alarm_type,level,operator,
                        threshold_value,threshold_min,threshold_max,recovery_threshold_value,duration_s,enabled
                 FROM alarm_protocol_point WHERE protocol_id=?1 ORDER BY point_id",
                )?;
            protocol.points = point_stmt
                .query_map(
                    params![protocol.protocol_id],
                    |item| {
                        Ok(AlarmProtocolPointRecord {
                            point_id: item.get(0)?,
                            protocol_id: item.get(1)?,
                            point_code: item.get(2)?,
                            point_name: item.get(3)?,
                            alarm_type: item.get(4)?,
                            level: item.get(5)?,
                            operator: item.get(6)?,
                            threshold_value: item.get(7)?,
                            threshold_min: item.get(8)?,
                            threshold_max: item.get(9)?,
                            recovery_threshold_value: item.get(10)?,
                            duration_s: item.get::<_, i64>(11)? as u64,
                            enabled: item.get::<_, i64>(12)? == 1,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            result.push(protocol);
        }
        Ok(result)
    }
}
