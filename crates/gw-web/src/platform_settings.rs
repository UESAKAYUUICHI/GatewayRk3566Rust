use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRequest {
    pub(super) gateway_id: String,
    pub(super) gateway_sn: String,
    #[serde(default)]
    pub(super) platform_http_url: String,
    pub(super) mqtt_host: String,
    pub(super) mqtt_port: u16,
    pub(super) mqtt_username: String,
    #[serde(default)]
    pub(super) mqtt_password: String,
    pub(super) heartbeat_s: Option<u64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlatformConfig {
    pub(super) gateway_id: String,
    pub(super) gateway_sn: String,
    pub(super) platform_http_url: String,
    pub(super) mqtt_host: String,
    pub(super) mqtt_port: u16,
    pub(super) mqtt_username: String,
    pub(super) password_configured: bool,
    pub(super) heartbeat_s: u64,
}
pub(super) async fn platform_config(
    State(state): State<WebState>,
) -> ApiResult<PlatformConfig> {
    let get = |key: &str, fallback: String| {
        state.store.get_config(key).ok().flatten().unwrap_or(fallback)
    };
    let password = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    Ok(
        Json(PlatformConfig {
            gateway_id: get("gateway_id", state.bootstrap.cloud.gateway_id.clone()),
            gateway_sn: get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone()),
            platform_http_url: get(
                "platform_http_url",
                state.bootstrap.cloud.platform_http_url.clone(),
            ),
            mqtt_host: get("mqtt_host", state.bootstrap.cloud.mqtt_host.clone()),
            mqtt_port: get("mqtt_port", state.bootstrap.cloud.mqtt_port.to_string())
                .parse()
                .unwrap_or(state.bootstrap.cloud.mqtt_port),
            mqtt_username: get(
                "mqtt_username",
                state.bootstrap.cloud.mqtt_username.clone(),
            ),
            password_configured: !password.is_empty(),
            heartbeat_s: get(
                    "heartbeat_s",
                    state.bootstrap.cloud.heartbeat_s.to_string(),
                )
                .parse()
                .unwrap_or(state.bootstrap.cloud.heartbeat_s),
        }),
    )
}
pub(super) async fn save_platform(
    State(state): State<WebState>,
    Json(body): Json<PlatformRequest>,
) -> ApiResult<Accepted> {
    if body.gateway_id.trim().is_empty() || body.gateway_sn.trim().is_empty()
        || body.mqtt_host.trim().is_empty()
    {
        return Err(ApiError::bad_request("网关标识和 MQTT 地址不能为空"));
    }
    let gateway_id = body.gateway_id.trim().to_string();
    let gateway_sn = body.gateway_sn.trim().to_string();
    let platform_http_url = if body.platform_http_url.trim().is_empty() {
        state.bootstrap.cloud.platform_http_url.clone()
    } else {
        body.platform_http_url.trim().trim_end_matches('/').to_string()
    };
    let mqtt_host = body.mqtt_host.trim().to_string();
    let mqtt_username = body.mqtt_username.trim().to_string();
    let password = if body.mqtt_password.is_empty() {
        state.store.get_config("mqtt_password").ok().flatten().unwrap_or_default()
    } else {
        body.mqtt_password
    };
    state
        .store
        .set_config("gateway_id", &gateway_id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("gateway_sn", &gateway_sn)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("platform_http_url", &platform_http_url)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("mqtt_host", &mqtt_host)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("mqtt_port", &body.mqtt_port.to_string())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("mqtt_username", &mqtt_username)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state
        .store
        .set_config("mqtt_password", &password)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    send_command(
            &state,
            UserCommand::SaveLink {
                gateway_id,
                gateway_sn,
                mqtt_host,
                mqtt_port: body.mqtt_port,
                mqtt_username,
                mqtt_password: password,
            },
        )
        .await?;
    if let Some(seconds) = body.heartbeat_s {
        state
            .store
            .set_config("heartbeat_s", &seconds.to_string())
            .map_err(|e| ApiError::internal(e.to_string()))?;
        send_command(
                &state,
                UserCommand::SetHeartbeat {
                    seconds,
                },
            )
            .await?;
    }
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn diagnose_platform(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    send_command(&state, UserCommand::DiagnoseCloud).await?;
    Ok(Json(Accepted { accepted: true }))
}
