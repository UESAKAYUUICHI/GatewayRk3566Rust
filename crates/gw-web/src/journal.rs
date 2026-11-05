use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub(super) id: String,
    pub(super) time: String,
    pub(super) level: String,
    pub(super) source: String,
    pub(super) message: String,
    pub(super) status: String,
    pub(super) delivery: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDto {
    pub(super) id: String,
    pub(super) time: String,
    pub(super) r#type: String,
    pub(super) target: String,
    pub(super) status: String,
    pub(super) message: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadDto {
    pub(super) id: i64,
    pub(super) time: String,
    pub(super) device_sn: String,
    pub(super) points: usize,
    pub(super) r#type: String,
    pub(super) access_status: String,
    pub(super) data_status: String,
    pub(super) latency: u64,
}
#[derive(Deserialize)]
pub(super) struct LimitQuery {
    pub(super) limit: Option<u32>,
}
pub(super) async fn events(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<EventDto>> {
    Ok(Json(load_events(&state.store, query.limit.unwrap_or(100).min(500))?))
}
pub(super) async fn acknowledge_all_events(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    state
        .store
        .acknowledge_active_alarms(now_ms())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn commands(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<CommandDto>> {
    Ok(Json(load_commands(&state.store, query.limit.unwrap_or(100).min(500))?))
}
pub(super) async fn uploads(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<UploadDto>> {
    Ok(Json(load_uploads(&state.store, query.limit.unwrap_or(100).min(500))?))
}
pub(super) fn load_events(store: &Store, limit: u32) -> Result<Vec<EventDto>, ApiError> {
    let alarms = store
        .recent_alarms(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let events = store
        .recent_events(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut combined: Vec<(u64, EventDto)> = alarms
        .into_iter()
        .map(|alarm| {
            let level = match alarm.level.as_str() {
                "CRITICAL" | "MAJOR" | "ERROR" => "ERROR",
                "MINOR" | "WARN" => "WARN",
                _ => "INFO",
            };
            (
                alarm.last_seen_ms,
                EventDto {
                    id: format!("alarm:{}:{}", alarm.source, alarm.source_event_id),
                    time: format_time(alarm.last_seen_ms),
                    level: level.into(),
                    source: if alarm.source == "PLATFORM" {
                        "平台告警".into()
                    } else {
                        "网关告警".into()
                    },
                    message: alarm.message,
                    status: alarm.status,
                    delivery: alarm.cloud_status,
                },
            )
        })
        .chain(
            events
                .into_iter()
                .enumerate()
                .map(|(index, e)| {
                    (
                        e.ts_ms,
                        EventDto {
                            id: format!("event:{}:{index}", e.ts_ms),
                            time: format_time(e.ts_ms),
                            level: e.level,
                            source: e.source,
                            message: e.message,
                            status: "LOG".into(),
                            delivery: "—".into(),
                        },
                    )
                }),
        )
        .collect();
    combined.sort_by_key(|(timestamp, _)| std::cmp::Reverse(*timestamp));
    combined.truncate(limit as usize);
    Ok(combined.into_iter().map(|(_, event)| event).collect())
}
pub(super) fn load_commands(
    store: &Store,
    limit: u32,
) -> Result<Vec<CommandDto>, ApiError> {
    Ok(
        store
            .recent_commands(limit)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .map(|c| CommandDto {
                id: c.command_id,
                time: format_time(c.received_ms),
                r#type: c.command_type,
                target: c.target_sn,
                status: normalize_status(&c.result_status),
                message: c.message,
            })
            .collect(),
    )
}
pub(super) fn load_uploads(
    store: &Store,
    limit: u32,
) -> Result<Vec<UploadDto>, ApiError> {
    let rows = store
        .recent_outbox(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(
        rows
            .into_iter()
            .map(|row| {
                let json: serde_json::Value = serde_json::from_slice(&row.payload)
                    .unwrap_or_default();
                let samples = json
                    .pointer("/data/meters")
                    .or_else(|| json.get("meters"))
                    .and_then(|v| v.as_array());
                let device_sn = samples
                    .and_then(|v| v.first())
                    .and_then(|v| v.get("deviceSn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("汇总报文")
                    .to_string();
                let points = samples
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| {
                                item.get("points")
                                    .and_then(|v| v.as_object())
                                    .map(|v| v.len())
                                    .unwrap_or(0)
                            })
                            .sum()
                    })
                    .unwrap_or(0);
                UploadDto {
                    id: row.id,
                    time: format_time(row.created_ms),
                    device_sn,
                    points,
                    r#type: "DATA_UPLOAD".into(),
                    access_status: upload_access_status(&row.status).into(),
                    data_status: upload_data_status(
                            &row.status,
                            row.last_error.as_deref(),
                        )
                        .into(),
                    latency: 0,
                }
            })
            .collect(),
    )
}
pub(super) fn upload_access_status(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "sent" => "FORWARDED",
        "pending" => "PENDING",
        "deferred" => "DEFERRED",
        _ => "FAILED",
    }
}
pub(super) fn upload_data_status(status: &str, error: Option<&str>) -> String {
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        return format!("ERROR: {error}");
    }
    match status.to_ascii_lowercase().as_str() {
        "sent" => "CONSUMED".into(),
        "pending" => "WAITING".into(),
        "deferred" => "CLOCK_WAIT".into(),
        _ => "UNKNOWN".into(),
    }
}
