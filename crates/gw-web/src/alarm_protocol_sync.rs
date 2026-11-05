use super::*;
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmProtocolSync {
    pub(super) protocols: Vec<PlatformAlarmProtocolDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmProtocolDto {
    pub(super) protocol_id: i64,
    pub(super) protocol_key: String,
    pub(super) protocol_name: String,
    pub(super) version_no: i64,
    #[serde(default = "default_true")]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) devices: Vec<PlatformAlarmProtocolDeviceDto>,
    #[serde(default)]
    pub(super) points: Vec<PlatformAlarmProtocolPointDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmProtocolDeviceDto {
    pub(super) device_sn: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformAlarmProtocolPointDto {
    pub(super) point_id: i64,
    pub(super) point_code: String,
    pub(super) point_name: String,
    pub(super) alarm_type: i32,
    pub(super) alarm_level: i32,
    pub(super) compare_operator: String,
    pub(super) threshold_value: Option<f64>,
    pub(super) threshold_min: Option<f64>,
    pub(super) threshold_max: Option<f64>,
    pub(super) recovery_threshold_value: Option<f64>,
    pub(super) duration_seconds: u64,
    pub(super) enabled: i32,
}
pub async fn sync_platform_alarm_protocols_once(
    state: &WebState,
) -> Result<usize, ApiError> {
    let get = |key: &str, fallback: String| {
        state.store.get_config(key).ok().flatten().unwrap_or(fallback)
    };
    let base = get("platform_http_url", state.bootstrap.cloud.platform_http_url.clone());
    let gateway_sn = get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone());
    let secret = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    if secret.is_empty() {
        return Err(
            ApiError::bad_request(
                "未配置网关密钥，不能同步平台告警协议",
            ),
        );
    }
    let response = reqwest::Client::new()
        .get(
            format!(
                "{}/api/platform/edge/config/alarm-protocols", base.trim_end_matches('/')
            ),
        )
        .header("X-Gateway-Sn", gateway_sn)
        .header("X-Gateway-Secret", secret)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(
            format!("平台告警协议同步失败: {error}"),
        ))?;
    let status = response.status();
    let envelope: PlatformEnvelope<PlatformAlarmProtocolSync> = response
        .json()
        .await
        .map_err(|error| ApiError::unavailable(
            format!("平台告警协议响应无法解析: {error}"),
        ))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(ApiError::unavailable(envelope.message));
    }
    let payload = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回告警协议数据"))?;
    let now = now_ms();
    let protocols = payload
        .protocols
        .into_iter()
        .map(|protocol| AlarmProtocolRecord {
            protocol_id: protocol.protocol_id,
            protocol_key: protocol.protocol_key,
            protocol_name: protocol.protocol_name,
            version_no: protocol.version_no,
            enabled: protocol.enabled,
            updated_ms: now,
            devices: protocol
                .devices
                .into_iter()
                .map(|device| AlarmProtocolDeviceRecord {
                    device_sn: device.device_sn,
                })
                .collect(),
            points: protocol
                .points
                .into_iter()
                .map(|point| AlarmProtocolPointRecord {
                    point_id: point.point_id,
                    protocol_id: protocol.protocol_id,
                    point_code: point.point_code,
                    point_name: point.point_name,
                    alarm_type: alarm_type_label(point.alarm_type),
                    level: alarm_level_label(point.alarm_level),
                    operator: point.compare_operator,
                    threshold_value: point.threshold_value,
                    threshold_min: point.threshold_min,
                    threshold_max: point.threshold_max,
                    recovery_threshold_value: point.recovery_threshold_value,
                    duration_s: point.duration_seconds,
                    enabled: point.enabled != 0,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    state
        .store
        .replace_platform_alarm_protocols(&protocols, now)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .store
        .set_config("alarm_protocol_last_sync_ms", &now.to_string())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(protocols.len())
}
pub(super) async fn sync_platform_alarm_protocols(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    sync_platform_alarm_protocols_once(&state).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn alarm_protocols(
    State(state): State<WebState>,
) -> ApiResult<serde_json::Value> {
    let rows = state
        .store
        .alarm_protocols()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let protocols = rows
        .into_iter()
        .map(|row| {
            serde_json::json!(
                { "protocolId" : row.protocol_id, "protocolKey" : row.protocol_key,
                "protocolName" : row.protocol_name, "versionNo" : row.version_no,
                "enabled" : row.enabled, "devices" : row.devices.into_iter().map(| device
                | serde_json::json!({ "deviceSn" : device.device_sn })).collect::< Vec <
                _ >> (), "points" : row.points.into_iter().map(| point |
                serde_json::json!({ "pointId" : point.point_id, "pointCode" : point
                .point_code, "pointName" : point.point_name, "alarmType" : point
                .alarm_type, "level" : point.level, "operator" : point.operator,
                "thresholdValue" : point.threshold_value, "thresholdMin" : point
                .threshold_min, "thresholdMax" : point.threshold_max,
                "recoveryThresholdValue" : point.recovery_threshold_value, "durationS" :
                point.duration_s, "enabled" : point.enabled })).collect::< Vec < _ >> ()
                }
            )
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({ "protocols" : protocols })))
}
