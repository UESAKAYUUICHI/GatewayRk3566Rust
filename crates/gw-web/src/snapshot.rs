use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySnapshotDto {
    pub(super) gateway_id: String,
    pub(super) gateway_sn: String,
    pub(super) version: String,
    pub(super) uptime: String,
    pub(super) cloud_online: bool,
    pub(super) mqtt_label: String,
    pub(super) clock_trusted: bool,
    pub(super) pending: u64,
    pub(super) sent: u64,
    pub(super) today_kwh: f64,
    pub(super) last_publish: String,
    pub(super) runtime: RuntimeDto,
    pub(super) wifi: WifiDto,
    pub(super) meters: Vec<MeterDto>,
    pub(super) events: Vec<EventDto>,
    pub(super) uploads: Vec<UploadDto>,
    pub(super) commands: Vec<CommandDto>,
    pub(super) networks: Vec<WifiNetworkDto>,
}
pub(super) async fn snapshot(
    State(state): State<WebState>,
) -> ApiResult<GatewaySnapshotDto> {
    Ok(Json(build_snapshot(&state).await?))
}
pub(super) async fn websocket(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket_loop(socket, state))
}
pub(super) async fn websocket_loop(mut socket: WebSocket, state: WebState) {
    let mut rx = state.snapshot_rx.clone();
    let Ok(mut data) = build_snapshot(&state).await else {
        return;
    };
    let mut last_slow_refresh = Instant::now();
    loop {
        let Ok(text) = serde_json::to_string(&data) else {
            return;
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
        tokio::select! {
            changed = rx.changed() => { if changed.is_err() { return; } let current = rx
            .borrow().clone(); refresh_dynamic(& mut data, & current, & state.store,
            state.runtime); if last_slow_refresh.elapsed() >= WEBSOCKET_SLOW_REFRESH {
            refresh_slow_fields(& mut data, & state). await; last_slow_refresh =
            Instant::now(); } }, incoming = socket.recv() => match incoming {
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return, _ => {} },
        }
    }
}
pub(super) async fn build_snapshot(
    state: &WebState,
) -> Result<GatewaySnapshotDto, ApiError> {
    let current = state.snapshot_rx.borrow().clone();
    let (net, scanned) = cached_network(state).await?;
    let get = |key: &str, fallback: String| {
        state.store.get_config(key).ok().flatten().unwrap_or(fallback)
    };
    Ok(GatewaySnapshotDto {
        gateway_id: get("gateway_id", state.bootstrap.cloud.gateway_id.clone()),
        gateway_sn: get("gateway_sn", state.bootstrap.cloud.gateway_sn.clone()),
        version: current.version.clone(),
        uptime: format_uptime(current.uptime_s(now_ms())),
        cloud_online: current.cloud == LinkHealth::Connected,
        mqtt_label: link_label(&current, state.runtime),
        clock_trusted: current.clock_trusted,
        pending: current.outbox_pending,
        sent: current.outbox_sent_total,
        today_kwh: current.today_kwh_total,
        last_publish: format_time(current.last_publish_ms),
        runtime: state.runtime,
        wifi: wifi_dto(net),
        meters: build_meter_list(&current, &state.store)?,
        events: load_events(&state.store, 100)?,
        uploads: load_uploads(&state.store, 100)?,
        commands: load_commands(&state.store, 100)?,
        networks: scanned.into_iter().map(network_dto).collect(),
    })
}
pub(super) async fn refresh_network(data: &mut GatewaySnapshotDto, state: &WebState) {
    if let Ok((net, networks)) = cached_network(state).await {
        data.wifi = wifi_dto(net);
        data.networks = networks.into_iter().map(network_dto).collect();
    }
}
pub(super) async fn refresh_slow_fields(
    data: &mut GatewaySnapshotDto,
    state: &WebState,
) {
    refresh_network(data, state).await;
    data.events = load_events(&state.store, 100).unwrap_or_default();
    data.uploads = load_uploads(&state.store, 100).unwrap_or_default();
    data.commands = load_commands(&state.store, 100).unwrap_or_default();
}
pub(super) fn refresh_dynamic(
    data: &mut GatewaySnapshotDto,
    current: &Snapshot,
    store: &Store,
    runtime: RuntimeDto,
) {
    data.version.clone_from(&current.version);
    data.uptime = format_uptime(current.uptime_s(now_ms()));
    data.cloud_online = current.cloud == LinkHealth::Connected;
    data.mqtt_label = link_label(current, runtime);
    data.clock_trusted = current.clock_trusted;
    data.pending = current.outbox_pending;
    data.sent = current.outbox_sent_total;
    data.today_kwh = current.today_kwh_total;
    data.last_publish = format_time(current.last_publish_ms);
    data.meters = build_meter_list(current, store)
        .unwrap_or_else(|_| {
            current.meters.iter().map(|meter| meter_dto(meter, store)).collect()
        });
}
pub(super) fn link_label(current: &Snapshot, _runtime: RuntimeDto) -> String {
    format!("MQTT {}", current.cloud.label())
}
