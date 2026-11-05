use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectPointDto {
    pub(super) code: String,
    pub(super) name: String,
    pub(super) value: f64,
    pub(super) unit: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectSampleDto {
    pub(super) id: i64,
    pub(super) date: String,
    pub(super) time: String,
    pub(super) timestamp_ms: u64,
    pub(super) meter_id: i64,
    pub(super) device_sn: String,
    pub(super) device_name: String,
    pub(super) channel_id: String,
    pub(super) profile: String,
    pub(super) modbus_addr: u8,
    pub(super) quality: u32,
    pub(super) quality_label: String,
    pub(super) quality_status: String,
    pub(super) point_count: usize,
    pub(super) summary: Vec<CollectPointDto>,
    pub(super) points: Vec<CollectPointDto>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectHistoryPageDto {
    pub(super) page_num: u32,
    pub(super) page_size: u32,
    pub(super) total: u64,
    pub(super) rows: Vec<CollectSampleDto>,
}
pub(super) async fn collection_history(
    State(state): State<WebState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<CollectHistoryPageDto> {
    let page_num = query.page_num.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(80).clamp(10, 200);
    let (rows, total) = state
        .store
        .recent_samples_page(page_num, page_size)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut result = Vec::with_capacity(rows.len());
    for sample in rows {
        result.push(collect_sample_dto(sample, &state.store)?);
    }
    Ok(
        Json(CollectHistoryPageDto {
            page_num,
            page_size,
            total,
            rows: result,
        }),
    )
}
pub(super) fn collect_sample_dto(
    sample: gw_store::SampleRecord,
    store: &Store,
) -> Result<CollectSampleDto, ApiError> {
    let meter = store
        .meter_by_sn(&sample.device_sn)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let (device_name, channel_id, profile) = meter
        .as_ref()
        .map(|record| {
            (
                if record.device_name.is_empty() {
                    record.device_sn.clone()
                } else {
                    record.device_name.clone()
                },
                record.channel_id.clone(),
                record.profile.clone(),
            )
        })
        .unwrap_or_else(|| (sample.device_sn.clone(), "--".into(), "--".into()));
    let raw_points = serde_json::from_str::<Vec<(String, f64)>>(&sample.points_json)
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mappings = if profile == "--" {
        Vec::new()
    } else {
        store
            .register_map(&profile)
            .map_err(|error| ApiError::internal(error.to_string()))?
    };
    let mut points = Vec::new();
    if mappings.is_empty() {
        for (code, value) in raw_points {
            points
                .push(CollectPointDto {
                    unit: default_unit(&code).into(),
                    name: code.clone(),
                    code,
                    value,
                });
        }
    } else {
        for mapping in mappings {
            if let Some(value) = raw_points.get(&mapping.point_code) {
                points
                    .push(CollectPointDto {
                        code: mapping.point_code.clone(),
                        name: if mapping.point_name.is_empty() {
                            mapping.point_code.clone()
                        } else {
                            mapping.point_name
                        },
                        value: *value,
                        unit: if mapping.unit.is_empty() {
                            default_unit(&mapping.point_code).into()
                        } else {
                            mapping.unit
                        },
                    });
            }
        }
    }
    let summary = points
        .iter()
        .filter(|point| {
            matches!(
                point.code.as_str(), "active_power_total" | "forward_active_energy" |
                "voltage_a" | "current_a" | "frequency"
            )
        })
        .take(4)
        .cloned()
        .collect::<Vec<_>>();
    Ok(CollectSampleDto {
        id: sample.id,
        date: format_date(sample.read_ms),
        time: format_time(sample.read_ms),
        timestamp_ms: sample.read_ms,
        meter_id: sample.meter_id,
        device_sn: sample.device_sn,
        device_name,
        channel_id,
        profile,
        modbus_addr: sample.modbus_addr,
        quality: sample.quality,
        quality_label: quality_label(sample.quality).into(),
        quality_status: quality_status(sample.quality).into(),
        point_count: points.len(),
        summary,
        points,
    })
}
pub(super) fn default_unit(code: &str) -> &'static str {
    match code.to_ascii_lowercase().as_str() {
        c if c.starts_with("voltage_") => "V",
        c if c.starts_with("current_") => "A",
        "active_power_total" => "kW",
        "reactive_power_total" => "kvar",
        "apparent_power_total" => "kVA",
        "forward_active_energy" => "kWh",
        "frequency" => "Hz",
        _ => "",
    }
}
pub(super) fn normalize_point_code(code: &str) -> String {
    code.trim().to_ascii_lowercase()
}
pub(super) fn quality_label(quality: u32) -> &'static str {
    match quality {
        0 => "有效",
        1 => "回退",
        2 => "缺失",
        3 => "异常",
        _ => "待确认",
    }
}
pub(super) fn quality_status(quality: u32) -> &'static str {
    match quality {
        0 => "ok",
        1 => "warn",
        2 | 3 => "error",
        _ => "warn",
    }
}
