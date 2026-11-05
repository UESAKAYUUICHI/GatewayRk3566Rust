use super::*;
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmSync {
    pub(super) alarms: Vec<PlatformAlarmDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmDto {
    pub(super) event_id: String,
    pub(super) alarm_type: String,
    pub(super) level: String,
    pub(super) device_sn: Option<String>,
    pub(super) point_code: Option<String>,
    pub(super) message: String,
    pub(super) alarm_time_ms: u64,
}
pub(super) async fn sync_platform_alarms(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    sync_platform_alarms_once(&state).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub async fn sync_platform_alarms_once(state: &WebState) -> Result<usize, ApiError> {
    let get = |key: &str, fallback: String| {
        state.store.get_config(key).ok().flatten().unwrap_or(fallback)
    };
    let base = get("platform_http_url", state.bootstrap.cloud.platform_http_url.clone());
    let gateway_sn = get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone());
    let secret = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    if secret.is_empty() {
        return Err(ApiError::bad_request("未配置网关密钥"));
    }
    let response = reqwest::Client::new()
        .get(format!("{}/api/platform/edge/config/alarms", base.trim_end_matches('/')))
        .header("X-Gateway-Sn", gateway_sn)
        .header("X-Gateway-Secret", secret)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(
            format!("平台告警同步失败: {error}"),
        ))?;
    let status = response.status();
    let envelope: PlatformEnvelope<PlatformAlarmSync> = response
        .json()
        .await
        .map_err(|error| ApiError::unavailable(
            format!("平台告警响应无法解析: {error}"),
        ))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(ApiError::unavailable(envelope.message));
    }
    let payload = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回告警数据"))?;
    let alarms = payload
        .alarms
        .into_iter()
        .map(|alarm| {
            let meter_id = alarm
                .device_sn
                .as_deref()
                .and_then(|sn| {
                    state.store.meter_by_sn(sn).ok().flatten().map(|meter| meter.id)
                });
            gw_store::AlarmEventRecord {
                source_event_id: alarm.event_id,
                source: "PLATFORM".into(),
                meter_id,
                alarm_type: alarm.alarm_type,
                level: alarm.level,
                point_code: alarm.point_code,
                message: alarm.message,
                status: "ACTIVE".into(),
                first_seen_ms: alarm.alarm_time_ms,
                last_seen_ms: alarm.alarm_time_ms,
                cloud_status: "SYNCED".into(),
                cloud_message_id: None,
                cloud_ack_ms: None,
            }
        })
        .collect::<Vec<_>>();
    state
        .store
        .replace_platform_alarms(&alarms, now_ms())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(alarms.len())
}
pub async fn sync_platform_alarm_rules_once(
    state: &WebState,
) -> Result<usize, ApiError> {
    sync_platform_alarm_protocols_once(state).await
}
pub(super) fn alarm_type_label(value: i32) -> String {
    match value {
        1 => "VOLTAGE",
        2 => "CURRENT",
        3 => "POWER",
        4 => "OFFLINE",
        _ => "GENERAL",
    }
        .into()
}
pub(super) fn alarm_level_label(value: i32) -> String {
    match value {
        4 => "CRITICAL",
        3 => "MAJOR",
        1 => "INFO",
        _ => "WARN",
    }
        .into()
}
