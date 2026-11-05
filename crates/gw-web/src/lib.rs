mod system_probe;
use system_probe::*;
mod system;
use system::*;
mod sync_types;
use sync_types::*;
mod snapshot;
pub use snapshot::{GatewaySnapshotDto};
use snapshot::*;
mod sync_validation;
use sync_validation::*;
mod platform_sync;
use platform_sync::*;
mod platform_settings;
pub use platform_settings::{PlatformRequest};
use platform_settings::*;
mod network;
pub use network::{WifiDto, WifiNetworkDto};
use network::*;
mod models;
use models::*;
mod meter_types;
pub use meter_types::{MeterDto, PointDto, MeterRequest, BindMeterRequest};
use meter_types::*;
mod meter_projection;
use meter_projection::*;
mod meter_commands;
use meter_commands::*;
mod journal;
pub use journal::{EventDto, CommandDto, UploadDto};
use journal::*;
mod energy;
pub use energy::{PowerPointDto};
use energy::*;
mod collection;
pub use collection::{CollectPointDto, CollectSampleDto, CollectHistoryPageDto};
use collection::*;
mod channels;
use channels::*;
mod camera_stream;
use camera_stream::*;
mod camera_capture;
use camera_capture::*;
mod alarm_sync;
pub use alarm_sync::{sync_platform_alarms_once, sync_platform_alarm_rules_once};
use alarm_sync::*;
mod alarm_rules;
pub use alarm_rules::{AlarmRuleDto, AlarmRuleConfigDto, AlarmRuleRequest, AlarmRuleSyncConfigRequest};
use alarm_rules::*;
mod alarm_protocol_sync;
pub use alarm_protocol_sync::{sync_platform_alarm_protocols_once};
use alarm_protocol_sync::*;
use std::{
    collections::BTreeMap, fs, path::{Path as FsPath, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use axum::{
    Json, Router, body::{Body, Bytes},
    extract::{Path, Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::StatusCode, response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Local};
use futures_util::stream;
use gw_agent::{BootstrapConfig, TransportRegistry, UserCommand};
use gw_core::snapshot::{LinkHealth, MeterSnapshot, Snapshot};
use gw_network::{NetworkManager, NetworkSnapshot, WifiNetwork};
use gw_store::{
    AlarmProtocolDeviceRecord, AlarmProtocolPointRecord, AlarmProtocolRecord,
    AlarmRuleRecord, MeterInput, MeterRecord, Store,
};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::process::Command as TokioCommand;
use tokio::sync::{mpsc, watch};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
#[derive(Clone)]
pub struct WebState {
    pub snapshot_rx: watch::Receiver<Snapshot>,
    pub command_tx: mpsc::Sender<UserCommand>,
    pub store: Arc<Store>,
    pub transports: Arc<TransportRegistry>,
    pub network: Arc<dyn NetworkManager>,
    pub network_cache: Arc<tokio::sync::Mutex<NetworkCache>>,
    pub bootstrap: BootstrapConfig,
    pub runtime: RuntimeDto,
}
#[derive(Debug, Default)]
pub struct NetworkCache {
    refreshed_at: Option<Instant>,
    snapshot: NetworkSnapshot,
    networks: Vec<WifiNetwork>,
}
const NETWORK_CACHE_TTL: Duration = Duration::from_secs(8);
const WEBSOCKET_SLOW_REFRESH: Duration = Duration::from_secs(8);
static CAMERA_CAPTURE_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDto {
    pub production: bool,
    pub transport: &'static str,
    pub cloud_link: &'static str,
    pub network: &'static str,
}
impl RuntimeDto {
    pub fn production() -> Self {
        Self {
            production: true,
            transport: "modbus-rtu",
            cloud_link: "mqtt-qos1",
            network: "networkmanager-nmcli",
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    code: &'static str,
    message: String,
}
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl ApiError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR",
            message: message.into(),
        }
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_ARGUMENT",
            message: message.into(),
        }
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "SERVICE_UNAVAILABLE",
            message: message.into(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}
type ApiResult<T> = Result<Json<T>, ApiError>;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    version: String,
    cloud_online: bool,
}
fn default_channel_id() -> String {
    "rs485-1".to_string()
}
#[derive(Serialize)]
struct Accepted {
    accepted: bool,
}
fn boolish<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(value) => Ok(value),
        serde_json::Value::Number(value) => Ok(value.as_i64().unwrap_or(0) != 0),
        serde_json::Value::String(value) => {
            let value = value.trim().to_ascii_lowercase();
            Ok(matches!(value.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled"))
        }
        serde_json::Value::Null => Ok(false),
        other => {
            Err(
                serde::de::Error::custom(
                    format!("expected bool/number/string, got {other}"),
                ),
            )
        }
    }
}
fn default_byte_order() -> String {
    "ABCD".to_string()
}
fn default_true() -> bool {
    true
}
fn default_timeout_ms() -> u64 {
    1_000
}
fn default_retry_count() -> u32 {
    2
}
fn default_poll_interval_seconds() -> u64 {
    300
}
pub fn router(state: WebState, web_root: PathBuf) -> Router {
    let index = web_root.join("index.html");
    let static_files = ServeDir::new(web_root).fallback(ServeFile::new(index));
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/camera/devices", get(camera_devices))
        .route("/api/v1/camera/stream", get(camera_stream))
        .route("/api/v1/camera/snapshot", get(camera_snapshot))
        .route("/api/v1/system/environment", get(system_environment))
        .route("/api/v1/system/diagnostics", get(system_diagnostics))
        .route("/api/v1/system/restart-service", post(restart_service))
        .route("/api/v1/system/shutdown-machine", post(shutdown_machine))
        .route("/api/v1/system/reboot-machine", post(reboot_machine))
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/v1/ws", get(websocket))
        .route("/api/v1/metrics/power24h", get(power24h))
        .route("/api/v1/metrics/weekly", get(weekly_metrics))
        .route("/api/v1/events", get(events))
        .route("/api/v1/events/acknowledge-all", post(acknowledge_all_events))
        .route("/api/v1/alarms/sync", post(sync_platform_alarms))
        .route("/api/v1/commands", get(commands))
        .route("/api/v1/uploads", get(uploads))
        .route("/api/v1/alarm-rules", get(alarm_rules).post(create_alarm_rule))
        .route("/api/v1/alarm-rules/sync", post(sync_platform_alarm_rules))
        .route("/api/v1/alarm-protocols/sync", post(sync_platform_alarm_protocols))
        .route("/api/v1/alarm-protocols", get(alarm_protocols))
        .route("/api/v1/alarm-rules/config", put(save_alarm_rule_sync_config))
        .route(
            "/api/v1/alarm-rules/{id}",
            put(update_alarm_rule).delete(delete_alarm_rule),
        )
        .route("/api/v1/meters", get(meters).post(create_meter))
        .route("/api/v1/meters/{id}", put(update_meter).delete(delete_meter))
        .route("/api/v1/meters/{id}/bind", post(bind_meter))
        .route("/api/v1/meters/{id}/read", post(read_meter))
        .route("/api/v1/meters/{id}/continuous-pull", post(set_continuous_pull))
        .route("/api/v1/collection/history", get(collection_history))
        .route("/api/v1/channels", get(channels).post(create_channel))
        .route("/api/v1/channels/{id}", put(save_channel).delete(delete_channel))
        .route("/api/v1/thing-models", get(thing_models))
        .route("/api/v1/thing-models/{profile}", delete(delete_thing_model))
        .route("/api/v1/network", get(network_snapshot))
        .route("/api/v1/network/scan", get(network_scan).post(network_scan))
        .route("/api/v1/network/connect", post(network_connect))
        .route("/api/v1/network/disconnect", post(network_disconnect))
        .route("/api/v1/network/forget", post(network_forget))
        .route("/api/v1/platform", get(platform_config).post(save_platform))
        .route("/api/v1/platform/diagnose", post(diagnose_platform))
        .route("/api/v1/config/sync", post(sync_platform_config))
        .fallback_service(static_files)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
async fn health(State(state): State<WebState>) -> ApiResult<HealthResponse> {
    let current = state.snapshot_rx.borrow().clone();
    Ok(
        Json(HealthResponse {
            status: "ok",
            version: current.version,
            cloud_online: current.cloud == LinkHealth::Connected,
        }),
    )
}
async fn send_command(state: &WebState, command: UserCommand) -> Result<(), ApiError> {
    state
        .command_tx
        .send(command)
        .await
        .map_err(|_| ApiError::unavailable("Agent 命令通道已关闭"))
}
fn normalize_status(value: &str) -> String {
    match value.to_ascii_uppercase().as_str() {
        "OK" | "SUCCESS" | "SUCCEEDED" => "SUCCESS".into(),
        "PENDING" | "RECEIVED" => "PENDING".into(),
        _ => "FAILED".into(),
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis() as u64)
        .unwrap_or(0)
}
fn format_time(ms: u64) -> String {
    if ms == 0 {
        return "--:--:--".into();
    }
    DateTime::<Local>::from(std::time::UNIX_EPOCH + Duration::from_millis(ms))
        .format("%H:%M:%S")
        .to_string()
}
fn format_date(ms: u64) -> String {
    if ms == 0 {
        return "----/--/--".into();
    }
    DateTime::<Local>::from(std::time::UNIX_EPOCH + Duration::from_millis(ms))
        .format("%Y/%m/%d")
        .to_string()
}
