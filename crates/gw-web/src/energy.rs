use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPointDto {
    pub(super) time: String,
    pub(super) timestamp_ms: u64,
    pub(super) device_sn: String,
    pub(super) device_name: String,
    pub(super) point: String,
    pub(super) value: f64,
    pub(super) unit: &'static str,
}
pub(super) async fn power24h(
    State(state): State<WebState>,
) -> ApiResult<Vec<PowerPointDto>> {
    let now = now_ms();
    let from = now.saturating_sub(24 * 60 * 60 * 1000);
    let rows = state
        .store
        .sample_history_since(from, 20_000)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .filter_map(|sample| {
            let points = serde_json::from_str::<Vec<(String, f64)>>(&sample.points_json)
                .ok()?;
            let value = points
                .into_iter()
                .find(|(code, _)| code.eq_ignore_ascii_case("active_power_total"))?
                .1;
            let device_name = state
                .store
                .meter_by_sn(&sample.device_sn)
                .ok()
                .flatten()
                .map(|m| {
                    if m.device_name.is_empty() { m.device_sn } else { m.device_name }
                })
                .unwrap_or_else(|| sample.device_sn.clone());
            Some(PowerPointDto {
                time: format_time(sample.read_ms),
                timestamp_ms: sample.read_ms,
                device_sn: sample.device_sn,
                device_name,
                point: "active_power_total".into(),
                value,
                unit: "kW",
            })
        })
        .collect();
    Ok(Json(rows))
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyMetricDto {
    pub(super) meter_id: i64,
    pub(super) device_sn: String,
    pub(super) device_name: String,
    pub(super) week_start_ms: u64,
    pub(super) week_end_ms: u64,
    pub(super) sample_count: u64,
    pub(super) normal_count: u64,
    pub(super) first_read_ms: u64,
    pub(super) last_read_ms: u64,
    pub(super) first_energy_kwh: Option<f64>,
    pub(super) last_energy_kwh: Option<f64>,
    pub(super) delta_kwh: f64,
    pub(super) avg_power_kw: Option<f64>,
    pub(super) min_voltage_v: Option<f64>,
    pub(super) max_voltage_v: Option<f64>,
}
pub(super) async fn weekly_metrics(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<WeeklyMetricDto>> {
    let now = now_ms();
    let from = now.saturating_sub(52 * 7 * 24 * 60 * 60 * 1000);
    let rows = state
        .store
        .weekly_summaries_since(from, query.limit.unwrap_or(520).min(2_000))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(
        Json(
            rows
                .into_iter()
                .map(|row| {
                    let device_name = if row.device_name.is_empty() {
                        row.device_sn.clone()
                    } else {
                        row.device_name.clone()
                    };
                    WeeklyMetricDto {
                        meter_id: row.meter_id,
                        device_sn: row.device_sn,
                        device_name,
                        week_start_ms: row.week_start_ms,
                        week_end_ms: row.week_end_ms,
                        sample_count: row.sample_count,
                        normal_count: row.normal_count,
                        first_read_ms: row.first_read_ms,
                        last_read_ms: row.last_read_ms,
                        first_energy_kwh: row.first_energy_kwh,
                        last_energy_kwh: row.last_energy_kwh,
                        delta_kwh: row.delta_kwh,
                        avg_power_kw: row.avg_power_kw,
                        min_voltage_v: row.min_voltage_v,
                        max_voltage_v: row.max_voltage_v,
                    }
                })
                .collect(),
        ),
    )
}
