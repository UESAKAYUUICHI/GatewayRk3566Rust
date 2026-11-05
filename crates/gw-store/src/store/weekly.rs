use super::*;
#[derive(Debug)]
pub(super) struct WeeklyAccumulator {
    pub(super) meter_id: i64,
    pub(super) sample_count: u64,
    pub(super) normal_count: u64,
    pub(super) first_read_ms: u64,
    pub(super) last_read_ms: u64,
    pub(super) first_energy_kwh: Option<f64>,
    pub(super) last_energy_kwh: Option<f64>,
    pub(super) power_sum_kw: f64,
    pub(super) power_count: u64,
    pub(super) min_voltage_v: Option<f64>,
    pub(super) max_voltage_v: Option<f64>,
}
impl WeeklyAccumulator {
    pub(super) fn new(meter_id: i64) -> Self {
        Self {
            meter_id,
            sample_count: 0,
            normal_count: 0,
            first_read_ms: 0,
            last_read_ms: 0,
            first_energy_kwh: None,
            last_energy_kwh: None,
            power_sum_kw: 0.0,
            power_count: 0,
            min_voltage_v: None,
            max_voltage_v: None,
        }
    }
    pub(super) fn push(&mut self, read_ms: u64, quality: u32, points_json: &str) {
        self.sample_count += 1;
        if quality == 0 {
            self.normal_count += 1;
        }
        if self.first_read_ms == 0 || read_ms < self.first_read_ms {
            self.first_read_ms = read_ms;
        }
        self.last_read_ms = self.last_read_ms.max(read_ms);
        if let Some(energy) = point_value(points_json, "forward_active_energy") {
            if self.first_energy_kwh.is_none() {
                self.first_energy_kwh = Some(energy);
            }
            self.last_energy_kwh = Some(energy);
        }
        if let Some(power) = point_value(points_json, "active_power_total") {
            self.power_sum_kw += power;
            self.power_count += 1;
        }
        for code in ["voltage_a", "voltage_b", "voltage_c"] {
            if let Some(voltage) = point_value(points_json, code) {
                self.min_voltage_v = Some(
                    self.min_voltage_v.map_or(voltage, |v| v.min(voltage)),
                );
                self.max_voltage_v = Some(
                    self.max_voltage_v.map_or(voltage, |v| v.max(voltage)),
                );
            }
        }
    }
    pub(super) fn delta_kwh(&self) -> f64 {
        match (self.first_energy_kwh, self.last_energy_kwh) {
            (Some(first), Some(last)) if last >= first => last - first,
            _ => 0.0,
        }
    }
    pub(super) fn avg_power_kw(&self) -> Option<f64> {
        (self.power_count > 0).then_some(self.power_sum_kw / self.power_count as f64)
    }
}
pub(super) fn point_value(points_json: &str, point_code: &str) -> Option<f64> {
    let points = serde_json::from_str::<Vec<(String, f64)>>(points_json).ok()?;
    points
        .into_iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(point_code))
        .map(|(_, value)| value)
}
impl Store {
    /// 将一个完整周区间的样本压缩成每表一行汇总，随后删除已上报或本地未上报样本。
    ///
    /// pending/deferred outbox 关联的样本不会被删除，避免断网期间数据丢失。
    pub fn archive_weekly_samples(
        &self,
        week_start_ms: u64,
        week_end_ms: u64,
        created_ms: u64,
    ) -> StoreResult<(usize, usize)> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let mut summaries: HashMap<i64, WeeklyAccumulator> = HashMap::new();
        {
            let mut stmt = tx
                .prepare(
                    "SELECT meter_id, read_ms, quality, points_json
                 FROM meter_sample
                 WHERE read_ms>=?1 AND read_ms<?2
                 ORDER BY meter_id, read_ms, id",
                )?;
            let mut rows = stmt
                .query(params![week_start_ms as i64, week_end_ms as i64])?;
            while let Some(row) = rows.next()? {
                let meter_id = row.get::<_, i64>(0)?;
                let read_ms = row.get::<_, i64>(1)? as u64;
                let quality = row.get::<_, i64>(2)? as u32;
                let points_json = row.get::<_, String>(3)?;
                summaries
                    .entry(meter_id)
                    .or_insert_with(|| WeeklyAccumulator::new(meter_id))
                    .push(read_ms, quality, &points_json);
            }
        }
        let mut summary_count = 0usize;
        for summary in summaries.values().filter(|summary| summary.sample_count > 0) {
            tx.execute(
                "INSERT INTO meter_weekly_summary(
                    meter_id,week_start_ms,week_end_ms,sample_count,normal_count,
                    first_read_ms,last_read_ms,first_energy_kwh,last_energy_kwh,
                    delta_kwh,avg_power_kw,min_voltage_v,max_voltage_v,created_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
                 ON CONFLICT(meter_id, week_start_ms) DO UPDATE SET
                    week_end_ms=excluded.week_end_ms,
                    sample_count=excluded.sample_count,
                    normal_count=excluded.normal_count,
                    first_read_ms=excluded.first_read_ms,
                    last_read_ms=excluded.last_read_ms,
                    first_energy_kwh=excluded.first_energy_kwh,
                    last_energy_kwh=excluded.last_energy_kwh,
                    delta_kwh=excluded.delta_kwh,
                    avg_power_kw=excluded.avg_power_kw,
                    min_voltage_v=excluded.min_voltage_v,
                    max_voltage_v=excluded.max_voltage_v,
                    created_ms=excluded.created_ms",
                params![
                    summary.meter_id, week_start_ms as i64, week_end_ms as i64, summary
                    .sample_count as i64, summary.normal_count as i64, summary
                    .first_read_ms as i64, summary.last_read_ms as i64, summary
                    .first_energy_kwh, summary.last_energy_kwh, summary.delta_kwh(),
                    summary.avg_power_kw(), summary.min_voltage_v, summary.max_voltage_v,
                    created_ms as i64,
                ],
            )?;
            summary_count += 1;
        }
        let deleted = tx
            .execute(
                "DELETE FROM meter_sample
             WHERE read_ms>=?1 AND read_ms<?2
               AND (
                    outbox_id IN (SELECT id FROM outbox_message WHERE status='sent')
                    OR (
                        outbox_id IS NULL
                        AND meter_id IN (SELECT id FROM meter WHERE upload_enabled=0)
                    )
               )",
                params![week_start_ms as i64, week_end_ms as i64],
            )?;
        tx.commit()?;
        Ok((summary_count, deleted))
    }
    pub fn weekly_summaries_since(
        &self,
        from_ms: u64,
        limit: u32,
    ) -> StoreResult<Vec<MeterWeeklySummary>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT s.meter_id,m.device_sn,m.device_name,s.week_start_ms,s.week_end_ms,
                    s.sample_count,s.normal_count,s.first_read_ms,s.last_read_ms,
                    s.first_energy_kwh,s.last_energy_kwh,s.delta_kwh,s.avg_power_kw,
                    s.min_voltage_v,s.max_voltage_v
             FROM meter_weekly_summary s
             JOIN meter m ON m.id=s.meter_id
             WHERE s.week_start_ms>=?1
             ORDER BY s.week_start_ms,s.meter_id
             LIMIT ?2",
            )?;
        let rows = stmt
            .query_map(
                params![from_ms as i64, limit],
                |r| {
                    Ok(MeterWeeklySummary {
                        meter_id: r.get(0)?,
                        device_sn: r.get(1)?,
                        device_name: r.get(2)?,
                        week_start_ms: r.get::<_, i64>(3)? as u64,
                        week_end_ms: r.get::<_, i64>(4)? as u64,
                        sample_count: r.get::<_, i64>(5)? as u64,
                        normal_count: r.get::<_, i64>(6)? as u64,
                        first_read_ms: r.get::<_, i64>(7)? as u64,
                        last_read_ms: r.get::<_, i64>(8)? as u64,
                        first_energy_kwh: r.get(9)?,
                        last_energy_kwh: r.get(10)?,
                        delta_kwh: r.get(11)?,
                        avg_power_kw: r.get(12)?,
                        min_voltage_v: r.get(13)?,
                        max_voltage_v: r.get(14)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
