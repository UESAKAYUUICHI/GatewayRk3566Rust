use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiDto {
    pub(super) connected: bool,
    pub(super) ssid: String,
    pub(super) signal: u8,
    pub(super) ipv4: String,
    pub(super) gateway: String,
    pub(super) dns: String,
    pub(super) interface_name: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetworkDto {
    pub(super) ssid: String,
    pub(super) signal: u8,
    pub(super) security: String,
    pub(super) connected: bool,
}
#[derive(Deserialize)]
pub(super) struct WifiConnectRequest {
    pub(super) ssid: String,
    #[serde(default)]
    pub(super) password: String,
}
#[derive(Deserialize)]
pub(super) struct WifiForgetRequest {
    pub(super) ssid: String,
}
pub(super) async fn network_snapshot(
    State(state): State<WebState>,
) -> ApiResult<WifiDto> {
    let (value, _) = cached_network(&state).await?;
    Ok(Json(wifi_dto(value)))
}
pub(super) async fn network_scan(
    State(state): State<WebState>,
) -> ApiResult<Vec<WifiNetworkDto>> {
    let manager = state.network.clone();
    let value = tokio::task::spawn_blocking(move || manager.scan())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(value.into_iter().map(network_dto).collect()))
}
pub(super) async fn network_connect(
    State(state): State<WebState>,
    Json(body): Json<WifiConnectRequest>,
) -> ApiResult<Accepted> {
    let manager = state.network.clone();
    tokio::task::spawn_blocking(move || manager.connect(&body.ssid, &body.password))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn network_disconnect(
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    let manager = state.network.clone();
    tokio::task::spawn_blocking(move || manager.disconnect())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn network_forget(
    State(state): State<WebState>,
    Json(body): Json<WifiForgetRequest>,
) -> ApiResult<Accepted> {
    let manager = state.network.clone();
    tokio::task::spawn_blocking(move || manager.forget(&body.ssid))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn cached_network(
    state: &WebState,
) -> Result<(NetworkSnapshot, Vec<WifiNetwork>), ApiError> {
    {
        let cache = state.network_cache.lock().await;
        if cache
            .refreshed_at
            .is_some_and(|refreshed_at| refreshed_at.elapsed() < NETWORK_CACHE_TTL)
        {
            return Ok((cache.snapshot.clone(), cache.networks.clone()));
        }
    }
    let manager = state.network.clone();
    let network = tokio::task::spawn_blocking(move || (
            manager.snapshot(),
            manager.cached_networks(),
        ))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let snapshot = network.0.unwrap_or_default();
    let networks = network.1.unwrap_or_default();
    let mut cache = state.network_cache.lock().await;
    cache.refreshed_at = Some(Instant::now());
    cache.snapshot = snapshot.clone();
    cache.networks = networks.clone();
    Ok((snapshot, networks))
}
pub(super) async fn invalidate_network_cache(state: &WebState) {
    let mut cache = state.network_cache.lock().await;
    cache.refreshed_at = None;
}
pub(super) fn wifi_dto(n: NetworkSnapshot) -> WifiDto {
    WifiDto {
        connected: !n.connected_ssid.is_empty(),
        ssid: n.connected_ssid,
        signal: n.signal,
        ipv4: n.ipv4,
        gateway: n.gateway,
        dns: n.dns,
        interface_name: n.interface,
    }
}
pub(super) fn network_dto(n: WifiNetwork) -> WifiNetworkDto {
    WifiNetworkDto {
        ssid: n.ssid,
        signal: n.signal,
        security: n.security,
        connected: n.connected,
    }
}
