use super::*;
use sha2::{Digest, Sha256};
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AlarmStrategyManifest {
    schema_version: u32,
    desired_revision: String,
    config_checksum: String,
    config_json: String,
}
fn verified_config(manifest: &AlarmStrategyManifest) -> Result<PlatformAlarmProtocolSync, String> {
    if manifest.schema_version != 1
        || manifest.desired_revision.len() != 64
        || !manifest
            .desired_revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("不支持的策略清单版本".into());
    }
    let checksum = format!("{:x}", Sha256::digest(manifest.config_json.as_bytes()));
    if checksum != manifest.config_checksum {
        return Err("策略内容校验失败".into());
    }
    let config: PlatformAlarmProtocolSync =
        serde_json::from_str(&manifest.config_json).map_err(|_| "策略内容无法解析".to_string())?;
    for protocol in &config.protocols {
        for point in &protocol.points {
            if point.evaluation_mode != "THRESHOLD"
                || point.recovery_samples != 1
                || point.duration_seconds != 0
            {
                return Err("网关不支持该策略判定方式".into());
            }
            let valid = if point.compare_operator == "between" {
                point
                    .threshold_min
                    .zip(point.threshold_max)
                    .is_some_and(|(min, max)| min.is_finite() && max.is_finite() && min <= max)
            } else {
                [">", ">=", "<", "<=", "="].contains(&point.compare_operator.as_str())
                    && point.threshold_value.is_some_and(f64::is_finite)
            };
            if !valid || point.point_code.is_empty() {
                return Err("策略阈值或测点无效".into());
            }
        }
    }
    Ok(config)
}
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
    pub(super) evaluation_mode: String,
    pub(super) recovery_samples: u32,
    pub(super) threshold_value: Option<f64>,
    pub(super) threshold_min: Option<f64>,
    pub(super) threshold_max: Option<f64>,
    pub(super) recovery_threshold_value: Option<f64>,
    pub(super) duration_seconds: u64,
    pub(super) enabled: i32,
}
pub async fn sync_platform_alarm_protocols_once(state: &WebState) -> Result<usize, ApiError> {
    let get = |key: &str, fallback: String| {
        state
            .store
            .get_config(key)
            .ok()
            .flatten()
            .unwrap_or(fallback)
    };
    let base = get(
        "platform_http_url",
        state.bootstrap.cloud.platform_http_url.clone(),
    );
    let gateway_sn = get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone());
    let secret = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    if secret.is_empty() {
        return Err(ApiError::bad_request(
            "未配置网关密钥，不能同步平台告警协议",
        ));
    }
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/platform/edge/config/alarm-protocols",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警协议同步失败: {error}")))?;
    let status = response.status();
    let envelope: PlatformEnvelope<AlarmStrategyManifest> = response
        .json()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警协议响应无法解析: {error}")))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(ApiError::unavailable(envelope.message));
    }
    let manifest = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回告警协议数据"))?;
    let payload = match verified_config(&manifest) {
        Ok(config) => config,
        Err(message) => {
            let _ = reqwest::Client::new()
                .post(format!("{}/api/platform/edge/config/alarm-protocols/ack", base.trim_end_matches('/')))
                .header("X-Gateway-Sn", &gateway_sn).header("X-Gateway-Secret", &secret)
                .json(&serde_json::json!({"desiredRevision":manifest.desired_revision,
                    "configChecksum":manifest.config_checksum,"status":"FAILED","errorMessage":message}))
                .timeout(Duration::from_secs(10)).send().await;
            return Err(ApiError::bad_request(message));
        }
    };
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
    let ack = reqwest::Client::new()
        .post(format!(
            "{}/api/platform/edge/config/alarm-protocols/ack",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .json(
            &serde_json::json!({"desiredRevision":manifest.desired_revision,
            "configChecksum":manifest.config_checksum,"status":"APPLIED"}),
        )
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(format!("策略应用回执失败: {error}")))?;
    let accepted: PlatformEnvelope<serde_json::Value> = ack
        .json()
        .await
        .map_err(|_| ApiError::unavailable("策略应用回执无法解析"))?;
    if accepted.code != 0 {
        return Err(ApiError::unavailable(accepted.message));
    }
    state
        .store
        .set_config(
            "alarm_strategy_applied_revision",
            &manifest.desired_revision,
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(protocols.len())
}
pub(super) async fn sync_platform_alarm_protocols(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    sync_platform_alarm_protocols_once(&state).await?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn alarm_protocols(State(state): State<WebState>) -> ApiResult<serde_json::Value> {
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

#[cfg(test)]
mod strategy_manifest_tests {
    use super::*;
    fn manifest(config: &str) -> AlarmStrategyManifest {
        AlarmStrategyManifest {
            schema_version: 1,
            desired_revision: "a".repeat(64),
            config_checksum: format!("{:x}", Sha256::digest(config.as_bytes())),
            config_json: config.into(),
        }
    }
    #[test]
    fn accepts_verified_empty_configuration_to_clear_old_rules() {
        assert!(verified_config(&manifest(r#"{"protocols":[]}"#)).is_ok());
    }
    #[test]
    fn rejects_tampering_and_unsupported_schema() {
        let mut value = manifest(r#"{"protocols":[]}"#);
        value.config_json.push(' ');
        assert!(verified_config(&value).is_err());
        value = manifest(r#"{"protocols":[]}"#);
        value.schema_version = 2;
        assert!(verified_config(&value).is_err());
    }
    #[test]
    fn refuses_advanced_rules_instead_of_running_threshold_fallback() {
        let config = r#"{"protocols":[{"protocolId":1,"protocolKey":"test","protocolName":"test","versionNo":1,"points":[{"pointId":1,"pointCode":"voltage_a","pointName":"voltage","alarmType":1,"alarmLevel":2,"compareOperator":">","evaluationMode":"N_OF_M","recoverySamples":1,"thresholdValue":245,"thresholdMin":null,"thresholdMax":null,"recoveryThresholdValue":240,"durationSeconds":0,"enabled":1}]}]}"#;
        assert!(verified_config(&manifest(config)).is_err());
        assert!(verified_config(&manifest(&config.replace("N_OF_M", "THRESHOLD"))).is_ok());
    }
}
