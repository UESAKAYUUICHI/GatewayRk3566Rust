use super::*;
impl Store {
    /// 累加某表某日增量。
    pub fn add_energy(
        &self,
        meter_id: i64,
        day: &str,
        delta_kwh: f64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO energy_daily(meter_id, day, delta_kwh) VALUES(?1, ?2, ?3)
             ON CONFLICT(meter_id, day) DO UPDATE SET delta_kwh = delta_kwh + excluded.delta_kwh",
            params![meter_id, day, delta_kwh],
        )?;
        Ok(())
    }
    pub fn energy_of(&self, meter_id: i64, day: &str) -> StoreResult<f64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value = conn
            .query_row(
                "SELECT delta_kwh FROM energy_daily WHERE meter_id=?1 AND day=?2",
                params![meter_id, day],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.unwrap_or(0.0))
    }
    pub fn energy_sum(&self, day: &str) -> StoreResult<f64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(delta_kwh), 0) FROM energy_daily WHERE day=?1",
                params![day],
                |r| r.get(0),
            )?;
        Ok(value)
    }
}
