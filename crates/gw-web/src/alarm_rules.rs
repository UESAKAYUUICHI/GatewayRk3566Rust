use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleDto {
    pub(super) id: i64,
    pub(super) rule_code: String,
    pub(super) source: String,
    pub(super) name: String,
    pub(super) level: String,
    pub(super) target_device_sn: Option<String>,
    pub(super) point_code: String,
    pub(super) operator: String,
    pub(super) threshold: f64,
    pub(super) unit: String,
    pub(super) duration_s: u64,
    pub(super) enabled: bool,
    pub(super) locked: bool,
    pub(super) updated: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleConfigDto {
    pub(super) sync_interval_s: u64,
    pub(super) last_sync: String,
    pub(super) rules: Vec<AlarmRuleDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleRequest {
    pub(super) rule_code: Option<String>,
    pub(super) name: String,
    pub(super) level: String,
    pub(super) target_device_sn: Option<String>,
    pub(super) point_code: String,
    pub(super) operator: String,
    pub(super) threshold: f64,
    #[serde(default)]
    pub(super) unit: String,
    pub(super) duration_s: u64,
    pub(super) enabled: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleSyncConfigRequest {
    pub(super) sync_interval_s: u64,
}
pub(super) async fn alarm_rules(
    State(state): State<WebState>,
) -> ApiResult<AlarmRuleConfigDto> {
    Ok(Json(load_alarm_rules(&state)?))
}
pub(super) async fn save_alarm_rule_sync_config(
    State(state): State<WebState>,
    Json(body): Json<AlarmRuleSyncConfigRequest>,
) -> ApiResult<Accepted> {
    let interval = body.sync_interval_s.clamp(30, 86_400);
    state
        .store
        .set_config("alarm_rule_sync_interval_s", &interval.to_string())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn create_alarm_rule(
    State(state): State<WebState>,
    Json(body): Json<AlarmRuleRequest>,
) -> Result<(StatusCode, Json<AlarmRuleDto>), ApiError> {
    let now = now_ms();
    let record = alarm_rule_record(None, body, now)?;
    let id = state
        .store
        .upsert_local_alarm_rule(None, &record)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let mut saved = record;
    saved.id = id;
    Ok((StatusCode::CREATED, Json(alarm_rule_dto(saved))))
}
pub(super) async fn update_alarm_rule(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<AlarmRuleRequest>,
) -> ApiResult<AlarmRuleDto> {
    let now = now_ms();
    let record = alarm_rule_record(Some(id), body, now)?;
    state
        .store
        .upsert_local_alarm_rule(Some(id), &record)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(alarm_rule_dto(record)))
}
pub(super) async fn delete_alarm_rule(
    Path(id): Path<i64>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    state
        .store
        .delete_local_alarm_rule(id)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn sync_platform_alarm_rules(
    State(state): State<WebState>,
) -> ApiResult<AlarmRuleConfigDto> {
    sync_platform_alarm_rules_once(&state).await?;
    Ok(Json(load_alarm_rules(&state)?))
}
pub(super) fn load_alarm_rules(
    state: &WebState,
) -> Result<AlarmRuleConfigDto, ApiError> {
    let sync_interval_s = state
        .store
        .get_config("alarm_rule_sync_interval_s")
        .map_err(|error| ApiError::internal(error.to_string()))?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(300);
    let last_sync = state
        .store
        .get_config("alarm_rule_last_sync_ms")
        .map_err(|error| ApiError::internal(error.to_string()))?
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_time)
        .unwrap_or_else(|| "--".into());
    let rules = state
        .store
        .alarm_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .map(alarm_rule_dto)
        .collect();
    Ok(AlarmRuleConfigDto {
        sync_interval_s,
        last_sync,
        rules,
    })
}
pub(super) fn alarm_rule_record(
    id: Option<i64>,
    body: AlarmRuleRequest,
    now: u64,
) -> Result<AlarmRuleRecord, ApiError> {
    let name = body.name.trim();
    let point_code = body.point_code.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("告警规则名称不能为空"));
    }
    if point_code.is_empty() {
        return Err(ApiError::bad_request("告警测点不能为空"));
    }
    let operator = body.operator.trim();
    if !matches!(operator, ">" | ">=" | "<" | "<=" | "==" | "!=") {
        return Err(
            ApiError::bad_request("告警比较符只支持 >、>=、<、<=、==、!="),
        );
    }
    let rule_code = body
        .rule_code
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("LOCAL-{}", now));
    Ok(AlarmRuleRecord {
        id: id.unwrap_or_default(),
        rule_code,
        source: "LOCAL".into(),
        name: name.into(),
        level: body.level,
        target_device_sn: body
            .target_device_sn
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        point_code: point_code.into(),
        operator: operator.into(),
        threshold: body.threshold,
        unit: body.unit,
        duration_s: body.duration_s.max(1),
        enabled: body.enabled,
        updated_ms: now,
    })
}
pub(super) fn alarm_rule_dto(row: AlarmRuleRecord) -> AlarmRuleDto {
    let locked = row.source == "PLATFORM";
    AlarmRuleDto {
        id: row.id,
        rule_code: row.rule_code,
        source: row.source,
        name: row.name,
        level: row.level,
        target_device_sn: row.target_device_sn,
        point_code: row.point_code,
        operator: row.operator,
        threshold: row.threshold,
        unit: row.unit,
        duration_s: row.duration_s,
        enabled: row.enabled,
        locked,
        updated: format_time(row.updated_ms),
    }
}
