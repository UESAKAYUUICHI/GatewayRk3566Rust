use std::{
    collections::BTreeMap,
    fs,
    path::{Path as FsPath, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes},
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Local};
use futures_util::stream;
use gw_agent::{BootstrapConfig, TransportRegistry, UserCommand};
use gw_core::snapshot::{LinkHealth, MeterSnapshot, Snapshot};
use gw_network::{NetworkManager, NetworkSnapshot, WifiNetwork};
use gw_store::{AlarmRuleRecord, MeterInput, MeterRecord, Store};
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
static CAMERA_CAPTURE_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemEnvironmentDto {
    hostname: String,
    os_release: String,
    kernel: String,
    architecture: String,
    cpu_model: String,
    cpu_cores: usize,
    cpu_temperature: String,
    load_average: String,
    memory: Vec<SystemInfoItemDto>,
    storage: Vec<SystemInfoItemDto>,
    runtime: Vec<SystemInfoItemDto>,
    kernel_params: Vec<SystemInfoItemDto>,
    boot_params: String,
    thermal_zones: Vec<SystemInfoItemDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInfoItemDto {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CameraDeviceDto {
    path: String,
    name: String,
    primary: bool,
    stream_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CameraStreamQuery {
    device: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiDto {
    connected: bool,
    ssid: String,
    signal: u8,
    ipv4: String,
    gateway: String,
    dns: String,
    interface_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetworkDto {
    ssid: String,
    signal: u8,
    security: String,
    connected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterDto {
    id: i64,
    sn: String,
    name: String,
    profile: String,
    model_version: String,
    channel_id: String,
    config_source: String,
    upload_enabled: bool,
    enabled: bool,
    collect_interval_s: u64,
    continuous_pull: bool,
    address: u8,
    online: bool,
    voltage: f64,
    current: f64,
    power: f64,
    energy: f64,
    last_read: String,
    points: Vec<PointDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointDto {
    code: String,
    name: String,
    value: Option<f64>,
    unit: String,
    quality: u32,
    collect_time: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    id: String,
    time: String,
    level: String,
    source: String,
    message: String,
    status: String,
    delivery: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDto {
    id: String,
    time: String,
    r#type: String,
    target: String,
    status: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadDto {
    id: i64,
    time: String,
    device_sn: String,
    points: usize,
    r#type: String,
    access_status: String,
    data_status: String,
    latency: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySnapshotDto {
    gateway_id: String,
    gateway_sn: String,
    version: String,
    uptime: String,
    cloud_online: bool,
    mqtt_label: String,
    clock_trusted: bool,
    pending: u64,
    sent: u64,
    today_kwh: f64,
    last_publish: String,
    runtime: RuntimeDto,
    wifi: WifiDto,
    meters: Vec<MeterDto>,
    events: Vec<EventDto>,
    uploads: Vec<UploadDto>,
    commands: Vec<CommandDto>,
    networks: Vec<WifiNetworkDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPointDto {
    time: String,
    timestamp_ms: u64,
    device_sn: String,
    device_name: String,
    point: String,
    value: f64,
    unit: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectPointDto {
    code: String,
    name: String,
    value: f64,
    unit: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectSampleDto {
    id: i64,
    date: String,
    time: String,
    timestamp_ms: u64,
    meter_id: i64,
    device_sn: String,
    device_name: String,
    channel_id: String,
    profile: String,
    modbus_addr: u8,
    quality: u32,
    quality_label: String,
    quality_status: String,
    point_count: usize,
    summary: Vec<CollectPointDto>,
    points: Vec<CollectPointDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectHistoryPageDto {
    page_num: u32,
    page_size: u32,
    total: u64,
    rows: Vec<CollectSampleDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    page_num: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleDto {
    id: i64,
    rule_code: String,
    source: String,
    name: String,
    level: String,
    target_device_sn: Option<String>,
    point_code: String,
    operator: String,
    threshold: f64,
    unit: String,
    duration_s: u64,
    enabled: bool,
    locked: bool,
    updated: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleConfigDto {
    sync_interval_s: u64,
    last_sync: String,
    rules: Vec<AlarmRuleDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleRequest {
    rule_code: Option<String>,
    name: String,
    level: String,
    target_device_sn: Option<String>,
    point_code: String,
    operator: String,
    threshold: f64,
    #[serde(default)]
    unit: String,
    duration_s: u64,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuleSyncConfigRequest {
    sync_interval_s: u64,
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterRequest {
    device_sn: String,
    #[serde(default)]
    device_name: String,
    modbus_addr: u8,
    profile: String,
    #[serde(default = "default_channel_id")]
    channel_id: String,
    #[serde(default)]
    upload_enabled: bool,
    collect_interval_s: u64,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindMeterRequest {
    channel_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContinuousPullRequest {
    enabled: bool,
}

fn default_channel_id() -> String {
    "rs485-1".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRequest {
    gateway_id: String,
    gateway_sn: String,
    #[serde(default)]
    platform_http_url: String,
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_username: String,
    #[serde(default)]
    mqtt_password: String,
    heartbeat_s: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformConfig {
    gateway_id: String,
    gateway_sn: String,
    platform_http_url: String,
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_username: String,
    password_configured: bool,
    heartbeat_s: u64,
}

#[derive(Deserialize)]
struct WifiConnectRequest {
    ssid: String,
    #[serde(default)]
    password: String,
}

#[derive(Deserialize)]
struct WifiForgetRequest {
    ssid: String,
}

#[derive(Serialize)]
struct Accepted {
    accepted: bool,
}

#[derive(Debug, Deserialize)]
struct PlatformEnvelope<T> {
    code: i32,
    message: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPayload {
    desired_revision: String,
    #[serde(default)]
    config_checksum: Option<String>,
    channels: Vec<SyncChannelDto>,
    devices: Vec<SyncDeviceDto>,
    models: Vec<SyncModelDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncChannelDto {
    channel_id: String,
    channel_name: String,
    protocol: String,
    serial_port: String,
    baud_rate: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: String,
    timeout_ms: u64,
    retry_count: u32,
    poll_interval_seconds: u64,
    #[serde(deserialize_with = "boolish")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncDeviceDto {
    platform_device_id: i64,
    device_sn: String,
    device_name: String,
    modbus_addr: u8,
    channel_id: String,
    collect_interval_s: u64,
    #[serde(deserialize_with = "boolish")]
    enabled: bool,
    model_version: String,
    profile_key: String,
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
        other => Err(serde::de::Error::custom(format!(
            "expected bool/number/string, got {other}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncModelDto {
    model_version_id: i64,
    profile_key: String,
    model_name: String,
    version: String,
    read_blocks: Vec<SyncReadBlockDto>,
    fields: Vec<SyncFieldDto>,
    bindings: Vec<SyncBindingDto>,
    #[serde(default)]
    commands: Vec<SyncCommandDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncReadBlockDto {
    id: i64,
    function_code: u8,
    start_address: u16,
    register_count: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncFieldDto {
    id: i64,
    read_block_id: i64,
    field_name: String,
    register_offset: u16,
    register_length: u16,
    value_type: String,
    #[serde(default = "default_byte_order")]
    byte_order: String,
    bit_offset: Option<u8>,
    bit_length: Option<u8>,
    decode_factor: f64,
    decode_offset: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncBindingDto {
    point_code: String,
    protocol_field_id: i64,
    canonical_factor: f64,
    canonical_offset: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncCommandDto {
    command_code: String,
    command_name: String,
    function_code: u8,
    register_address: u16,
    encode_type: String,
    fixed_value: Option<u16>,
    parameter_json: Option<serde_json::Value>,
}

fn default_byte_order() -> String {
    "ABCD".to_string()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncResult {
    applied: bool,
    revision: String,
    channels: usize,
    devices: usize,
    models: usize,
    cloud_acknowledged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformAlarmSync {
    alarms: Vec<PlatformAlarmDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformAlarmDto {
    event_id: String,
    alarm_type: String,
    level: String,
    device_sn: Option<String>,
    point_code: Option<String>,
    message: String,
    alarm_time_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformAlarmRuleSync {
    rules: Vec<PlatformAlarmRuleDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformAlarmRuleDto {
    rule_code: String,
    name: String,
    level: String,
    device_sn: Option<String>,
    point_code: String,
    operator: String,
    threshold: f64,
    #[serde(default)]
    unit: String,
    duration_s: u64,
    enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDto {
    id: String,
    name: String,
    protocol: String,
    port: String,
    baud: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: String,
    timeout_ms: u64,
    retry_count: u32,
    poll_interval_seconds: u64,
    config_source: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelRequest {
    name: String,
    port: String,
    baud: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_retry_count")]
    retry_count: u32,
    #[serde(default = "default_poll_interval_seconds")]
    poll_interval_seconds: u64,
    enabled: bool,
}

fn default_timeout_ms() -> u64 { 1_000 }
fn default_retry_count() -> u32 { 2 }
fn default_poll_interval_seconds() -> u64 { 300 }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThingModelDto {
    profile: String,
    name: String,
    version: String,
    source: String,
    point_count: u32,
    device_count: u32,
    points: Vec<ThingPointDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThingPointDto {
    code: String,
    name: String,
    unit: String,
    function_code: u8,
    address: u16,
    quantity: u16,
    field_offset: u16,
    field_quantity: u16,
    bit_offset: Option<u8>,
    bit_length: Option<u8>,
    data_type: String,
    byte_order: String,
    scale: f64,
    offset: f64,
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
        .route("/api/v1/events", get(events))
        .route(
            "/api/v1/events/acknowledge-all",
            post(acknowledge_all_events),
        )
        .route("/api/v1/alarms/sync", post(sync_platform_alarms))
        .route("/api/v1/commands", get(commands))
        .route("/api/v1/uploads", get(uploads))
        .route(
            "/api/v1/alarm-rules",
            get(alarm_rules).post(create_alarm_rule),
        )
        .route("/api/v1/alarm-rules/sync", post(sync_platform_alarm_rules))
        .route(
            "/api/v1/alarm-rules/config",
            put(save_alarm_rule_sync_config),
        )
        .route(
            "/api/v1/alarm-rules/{id}",
            put(update_alarm_rule).delete(delete_alarm_rule),
        )
        .route("/api/v1/meters", get(meters).post(create_meter))
        .route(
            "/api/v1/meters/{id}",
            put(update_meter).delete(delete_meter),
        )
        .route("/api/v1/meters/{id}/bind", post(bind_meter))
        .route("/api/v1/meters/{id}/read", post(read_meter))
        .route(
            "/api/v1/meters/{id}/continuous-pull",
            post(set_continuous_pull),
        )
        .route("/api/v1/collection/history", get(collection_history))
        .route("/api/v1/channels", get(channels).post(create_channel))
        .route(
            "/api/v1/channels/{id}",
            put(save_channel).delete(delete_channel),
        )
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

async fn sync_platform_alarms(State(state): State<WebState>) -> ApiResult<Accepted> {
    sync_platform_alarms_once(&state).await?;
    Ok(Json(Accepted { accepted: true }))
}

pub async fn sync_platform_alarms_once(state: &WebState) -> Result<usize, ApiError> {
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
        return Err(ApiError::bad_request("未配置网关密钥"));
    }
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/platform/edge/config/alarms",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", gateway_sn)
        .header("X-Gateway-Secret", secret)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警同步失败: {error}")))?;
    let status = response.status();
    let envelope: PlatformEnvelope<PlatformAlarmSync> = response
        .json()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警响应无法解析: {error}")))?;
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
            let meter_id = alarm.device_sn.as_deref().and_then(|sn| {
                state
                    .store
                    .meter_by_sn(sn)
                    .ok()
                    .flatten()
                    .map(|meter| meter.id)
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

pub async fn sync_platform_alarm_rules_once(state: &WebState) -> Result<usize, ApiError> {
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
            "未配置网关密钥，不能同步平台告警规则",
        ));
    }
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/platform/edge/config/alarm-rules",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", gateway_sn)
        .header("X-Gateway-Secret", secret)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警规则同步失败: {error}")))?;
    let status = response.status();
    let envelope: PlatformEnvelope<PlatformAlarmRuleSync> = response
        .json()
        .await
        .map_err(|error| ApiError::unavailable(format!("平台告警规则响应无法解析: {error}")))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(ApiError::unavailable(envelope.message));
    }
    let payload = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回告警规则数据"))?;
    let now = now_ms();
    let rules = payload
        .rules
        .into_iter()
        .map(|rule| AlarmRuleRecord {
            id: 0,
            rule_code: rule.rule_code,
            source: "PLATFORM".into(),
            name: rule.name,
            level: rule.level,
            target_device_sn: rule.device_sn,
            point_code: rule.point_code,
            operator: rule.operator,
            threshold: rule.threshold,
            unit: rule.unit,
            duration_s: rule.duration_s,
            enabled: rule.enabled,
            updated_ms: now,
        })
        .collect::<Vec<_>>();
    state
        .store
        .replace_platform_alarm_rules(&rules, now)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .store
        .set_config("alarm_rule_last_sync_ms", &now.to_string())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(rules.len())
}

async fn health(State(state): State<WebState>) -> ApiResult<HealthResponse> {
    let current = state.snapshot_rx.borrow().clone();
    Ok(Json(HealthResponse {
        status: "ok",
        version: current.version,
        cloud_online: current.cloud == LinkHealth::Connected,
    }))
}

async fn camera_devices() -> ApiResult<Vec<CameraDeviceDto>> {
    Ok(Json(discover_camera_devices()))
}

async fn camera_stream(Query(query): Query<CameraStreamQuery>) -> Result<Response, ApiError> {
    stop_camera_live_processes().await;
    let device = select_camera_device(&query)?;
    let width = query.width.unwrap_or(640).clamp(160, 1280);
    let height = query.height.unwrap_or(480).clamp(120, 720);
    let fps = query.fps.unwrap_or(10).clamp(1, 20);
    let fps_text = fps.to_string();
    let device_arg = format!("device={}", device.path);
    let caps_arg = format!("video/x-raw,format=NV12,width={width},height={height},framerate={fps_text}/1");
    let capture_id = format!(
        "park-gateway-camera-live-{}-{}",
        std::process::id(),
        Local::now().timestamp_millis()
    );
    let output_pattern = format!("/tmp/{capture_id}-%05d.jpg");
    let _ = TokioCommand::new("v4l2-ctl")
        .args([
            "-d",
            &device.path,
            "-c",
            "auto_exposure=0,gain_automatic=1,white_balance_automatic=1",
        ])
        .output()
        .await;
    let mut command = TokioCommand::new("gst-launch-1.0");
    command
        .args([
            "-q",
            "v4l2src",
            &device_arg,
            "!",
            &caps_arg,
            "!",
            "jpegenc",
            "!",
            "multifilesink",
            &format!("location={output_pattern}"),
            "max-files=32",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| ApiError::unavailable(format!("摄像头流启动失败: {error}")))?;
    struct CameraLiveState {
        child: tokio::process::Child,
        capture_id: String,
        last_index: u32,
    }
    let stream = stream::unfold(
        CameraLiveState {
            child,
            capture_id,
            last_index: 2,
        },
        |mut state| async move {
            loop {
                if matches!(state.child.try_wait(), Ok(Some(_))) {
                    return None;
                }
                if let Some((index, path)) =
                    latest_camera_live_frame(&state.capture_id, state.last_index)
                {
                    let image = fs::read(&path).unwrap_or_default();
                    let _ = fs::remove_file(&path);
                    cleanup_camera_live_frames(&state.capture_id, index);
                    state.last_index = index;
                    if !image.is_empty() {
                        let mut part = Vec::with_capacity(image.len() + 128);
                        part.extend_from_slice(b"--park-gateway-camera\r\n");
                        part.extend_from_slice(b"Content-Type: image/jpeg\r\n");
                        part.extend_from_slice(
                            format!("Content-Length: {}\r\n\r\n", image.len()).as_bytes(),
                        );
                        part.extend_from_slice(&image);
                        part.extend_from_slice(b"\r\n");
                        return Some((Ok::<Bytes, std::io::Error>(Bytes::from(part)), state));
                    }
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        },
    );
    let body = Body::from_stream(stream);
    Ok((
        [
            ("content-type", "multipart/x-mixed-replace; boundary=park-gateway-camera"),
            ("cache-control", "no-store"),
            ("x-accel-buffering", "no"),
        ],
        body,
    )
        .into_response())
}

fn latest_camera_live_frame(capture_id: &str, last_index: u32) -> Option<(u32, PathBuf)> {
    let prefix = format!("{capture_id}-");
    fs::read_dir("/tmp")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpg") {
                return None;
            }
            let index = file_name
                .trim_start_matches(&prefix)
                .trim_end_matches(".jpg")
                .parse::<u32>()
                .ok()?;
            (index > last_index).then_some((index, entry.path()))
        })
        .max_by_key(|(index, _)| *index)
}

fn cleanup_camera_live_frames(capture_id: &str, keep_from: u32) {
    let prefix = format!("{capture_id}-");
    let Ok(entries) = fs::read_dir("/tmp") else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpg") {
            continue;
        }
        let Ok(index) = file_name
            .trim_start_matches(&prefix)
            .trim_end_matches(".jpg")
            .parse::<u32>()
        else {
            continue;
        };
        if index < keep_from {
            let _ = fs::remove_file(entry.path());
        }
    }
}

async fn camera_snapshot(Query(query): Query<CameraStreamQuery>) -> Result<Response, ApiError> {
    stop_camera_live_processes().await;
    let image = capture_camera_frame(&query, 12).await?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "no-store"),
        ],
        image,
    )
        .into_response())
}

async fn stop_camera_live_processes() {
    let _ = TokioCommand::new("sh")
        .args([
            "-c",
            "for pid in $(pgrep -x gst-launch-1.0 2>/dev/null); do args=$(tr '\\0' ' ' < /proc/$pid/cmdline 2>/dev/null || true); case \"$args\" in *'park-gateway-camera-live-'*) kill \"$pid\" 2>/dev/null || true ;; esac; done",
        ])
        .output()
        .await;
}

fn select_camera_device(query: &CameraStreamQuery) -> Result<CameraDeviceDto, ApiError> {
    let devices = discover_camera_devices();
    if devices.is_empty() {
        return Err(ApiError::unavailable("未发现可用摄像头设备"));
    }
    query
        .device
        .as_deref()
        .and_then(|path| devices.iter().find(|item| item.path == path))
        .or_else(|| devices.iter().find(|item| item.primary))
        .or_else(|| devices.first())
        .cloned()
        .ok_or_else(|| ApiError::unavailable("未发现可用摄像头设备"))
}

async fn capture_camera_frame(query: &CameraStreamQuery, buffers: u32) -> Result<Vec<u8>, ApiError> {
    let _capture_guard = CAMERA_CAPTURE_LOCK.lock().await;
    let device = select_camera_device(query)?;
    let width = query.width.unwrap_or(640).clamp(160, 1280);
    let height = query.height.unwrap_or(480).clamp(120, 720);
    let fps = query.fps.unwrap_or(10).clamp(1, 20);
    let fps_text = fps.to_string();
    let device_arg = format!("device={}", device.path);
    let caps_arg = format!("video/x-raw,format=NV12,width={width},height={height},framerate={fps_text}/1");
    let _ = TokioCommand::new("v4l2-ctl")
        .args([
            "-d",
            &device.path,
            "-c",
            "auto_exposure=0,gain_automatic=1,white_balance_automatic=1",
        ])
        .output()
        .await;
    let capture_id = format!(
        "park-gateway-camera-{}-{}",
        std::process::id(),
        Local::now().timestamp_millis()
    );
    let output_pattern = format!("/tmp/{capture_id}-%02d.jpg");
    let mut command = TokioCommand::new("gst-launch-1.0");
    command
        .args([
            "-q",
            "v4l2src",
            &device_arg,
            &format!("num-buffers={}", buffers.max(1)),
            "!",
            &caps_arg,
            "!",
            "jpegenc",
            "!",
            "multifilesink",
            &format!("location={output_pattern}"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(8), command.output())
    .await
    .map_err(|_| ApiError::unavailable("摄像头抓图超时"))?
    .map_err(|error| ApiError::unavailable(format!("摄像头抓图失败: {error}")))?;
    let latest_path = (0..12)
        .rev()
        .map(|index| PathBuf::from(format!("/tmp/{capture_id}-{index:02}.jpg")))
        .find(|path| path.exists());
    let image = latest_path
        .as_ref()
        .and_then(|path| fs::read(path).ok())
        .unwrap_or_default();
    for index in 0..12 {
        let _ = fs::remove_file(format!("/tmp/{capture_id}-{index:02}.jpg"));
    }
    if !output.status.success() || image.is_empty() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::unavailable(format!(
            "摄像头没有输出画面{}",
            if message.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", message.trim())
            }
        )));
    }
    Ok(image)
}

fn discover_camera_devices() -> Vec<CameraDeviceDto> {
    let mut devices = Vec::new();
    let entries = match fs::read_dir("/sys/class/video4linux") {
        Ok(entries) => entries,
        Err(_) => return devices,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with("video") {
            continue;
        }
        let path = format!("/dev/{file_name}");
        let name = fs::read_to_string(entry.path().join("name"))
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|_| file_name.clone());
        let lower = name.to_ascii_lowercase();
        let looks_like_capture = lower.contains("mainpath")
            || lower.contains("selfpath")
            || lower.contains("camera")
            || lower.contains("uvc")
            || lower.contains("ov");
        if !looks_like_capture {
            continue;
        }
        let primary = file_name == "video0" || lower.contains("mainpath");
        devices.push(CameraDeviceDto {
            stream_url: format!("/api/v1/camera/stream?device={path}"),
            path,
            name,
            primary,
        });
    }
    if devices.iter().any(|device| {
        device.path == "/dev/video0" || device.name.to_ascii_lowercase().contains("mainpath")
    }) {
        devices.retain(|device| {
            device.path == "/dev/video0" || device.name.to_ascii_lowercase().contains("mainpath")
        });
    }
    devices.sort_by_key(|device| (!device.primary, device.path.clone()));
    devices
}

async fn snapshot(State(state): State<WebState>) -> ApiResult<GatewaySnapshotDto> {
    Ok(Json(build_snapshot(&state).await?))
}

async fn system_environment() -> ApiResult<SystemEnvironmentDto> {
    Ok(Json(collect_system_environment().await))
}

async fn collect_system_environment() -> SystemEnvironmentDto {
    let hostname = read_trim("/proc/sys/kernel/hostname").unwrap_or_else(|| "--".into());
    let os_release = read_os_release().unwrap_or_else(|| "--".into());
    let kernel = command_output("uname", &["-r"]).await;
    let architecture = command_output("uname", &["-m"]).await;
    let cpu_model = read_cpu_model().await.unwrap_or_else(|| "--".into());
    let cpu_cores = fs::read_to_string("/proc/cpuinfo")
        .map(|text| {
            text.lines()
                .filter(|line| line.starts_with("processor"))
                .count()
        })
        .unwrap_or(0);
    let thermal_zones = read_thermal_zones();
    let cpu_temperature = thermal_zones
        .iter()
        .find(|item| item.key.to_lowercase().contains("cpu"))
        .or_else(|| thermal_zones.first())
        .map(|item| item.value.clone())
        .unwrap_or_else(|| "--".into());
    let load_average = read_trim("/proc/loadavg").unwrap_or_else(|| "--".into());
    let memory = read_memory_info();
    let storage = read_storage_info().await;
    let runtime = vec![
        item("当前用户", &command_output("whoami", &[]).await),
        item("系统时间", &command_output("date", &["+%F %T %Z"]).await),
        item("启动时间", &command_output("uptime", &["-p"]).await),
        item(
            "进程数量",
            &command_output("sh", &["-c", "ls /proc | grep -E '^[0-9]+$' | wc -l"]).await,
        ),
    ];
    let kernel_params = read_kernel_params().await;
    let boot_params = read_trim("/proc/cmdline").unwrap_or_else(|| "--".into());
    SystemEnvironmentDto {
        hostname,
        os_release,
        kernel,
        architecture,
        cpu_model,
        cpu_cores,
        cpu_temperature,
        load_average,
        memory,
        storage,
        runtime,
        kernel_params,
        boot_params,
        thermal_zones,
    }
}

async fn system_diagnostics(State(state): State<WebState>) -> Result<Response, ApiError> {
    let environment = collect_system_environment().await;
    let snapshot = build_snapshot(&state).await?;
    let report = format!(
        "# Park Gateway Diagnostics\n\n## Snapshot\n{}\n\n## Environment\n{}\n",
        serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".into()),
        serde_json::to_string_pretty(&environment).unwrap_or_else(|_| "{}".into()),
    );
    Ok((
        [
            ("content-type", "text/plain; charset=utf-8"),
            (
                "content-disposition",
                "attachment; filename=\"park-gateway-diagnostics.txt\"",
            ),
        ],
        report,
    )
        .into_response())
}

async fn restart_service() -> ApiResult<Accepted> {
    let pid = std::process::id();
    let script = format!(
        "(sleep 1; systemctl restart park-gateway.service || kill -TERM {pid}) >/tmp/park-gateway-service-restart.log 2>&1 &"
    );
    Command::new("sh")
        .args(["-c", &script])
        .spawn()
        .map_err(|error| ApiError::internal(format!("无法发起服务重启: {error}")))?;
    Ok(Json(Accepted { accepted: true }))
}

async fn shutdown_machine() -> ApiResult<Accepted> {
    schedule_power_action(
        "systemctl poweroff",
        "/tmp/park-gateway-poweroff.log",
        "关机",
    )
}

async fn reboot_machine() -> ApiResult<Accepted> {
    schedule_power_action(
        "systemctl reboot",
        "/tmp/park-gateway-reboot.log",
        "重启机器",
    )
}

fn schedule_power_action(command: &str, log_path: &str, label: &str) -> ApiResult<Accepted> {
    let script = format!("(sleep 1; {command}) >{log_path} 2>&1 &");
    Command::new("sh")
        .args(["-c", &script])
        .spawn()
        .map_err(|error| ApiError::internal(format!("无法发起{label}: {error}")))?;
    Ok(Json(Accepted { accepted: true }))
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<WebState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket_loop(socket, state))
}

async fn websocket_loop(mut socket: WebSocket, state: WebState) {
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
            changed = rx.changed() => {
                if changed.is_err() { return; }
                let current = rx.borrow().clone();
                refresh_dynamic(&mut data, &current, &state.store, state.runtime);
                if last_slow_refresh.elapsed() >= WEBSOCKET_SLOW_REFRESH {
                    refresh_slow_fields(&mut data, &state).await;
                    last_slow_refresh = Instant::now();
                }
            },
            incoming = socket.recv() => match incoming { Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return, _ => {} },
        }
    }
}

async fn events(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<EventDto>> {
    Ok(Json(load_events(
        &state.store,
        query.limit.unwrap_or(100).min(500),
    )?))
}

async fn acknowledge_all_events(State(state): State<WebState>) -> ApiResult<Accepted> {
    state
        .store
        .acknowledge_active_alarms(now_ms())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}

async fn power24h(State(state): State<WebState>) -> ApiResult<Vec<PowerPointDto>> {
    let now = now_ms();
    let from = now.saturating_sub(24 * 60 * 60 * 1000);
    let rows = state
        .store
        .sample_history_since(from, 20_000)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .filter_map(|sample| {
            let points = serde_json::from_str::<Vec<(String, f64)>>(&sample.points_json).ok()?;
            let value = points
                .into_iter()
                .find(|(code, _)| code.eq_ignore_ascii_case("active_power_total"))?
                .1;
            let device_name = state
                .store
                .meter_by_sn(&sample.device_sn)
                .ok()
                .flatten()
                .map(|m| {
                    if m.device_name.is_empty() {
                        m.device_sn
                    } else {
                        m.device_name
                    }
                })
                .unwrap_or_else(|| sample.device_sn.clone());
            Some(PowerPointDto {
                time: format_time(sample.read_ms),
                timestamp_ms: sample.read_ms,
                device_sn: sample.device_sn,
                device_name,
                point: "active_power_total".into(),
                value,
                unit: "kW",
            })
        })
        .collect();
    Ok(Json(rows))
}

async fn commands(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<CommandDto>> {
    Ok(Json(load_commands(
        &state.store,
        query.limit.unwrap_or(100).min(500),
    )?))
}

async fn uploads(
    State(state): State<WebState>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Vec<UploadDto>> {
    Ok(Json(load_uploads(
        &state.store,
        query.limit.unwrap_or(100).min(500),
    )?))
}

async fn alarm_rules(State(state): State<WebState>) -> ApiResult<AlarmRuleConfigDto> {
    Ok(Json(load_alarm_rules(&state)?))
}

async fn save_alarm_rule_sync_config(
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

async fn create_alarm_rule(
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

async fn update_alarm_rule(
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

async fn delete_alarm_rule(
    Path(id): Path<i64>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    state
        .store
        .delete_local_alarm_rule(id)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}

async fn sync_platform_alarm_rules(State(state): State<WebState>) -> ApiResult<AlarmRuleConfigDto> {
    sync_platform_alarm_rules_once(&state).await?;
    Ok(Json(load_alarm_rules(&state)?))
}

async fn meters(State(state): State<WebState>) -> ApiResult<Vec<MeterDto>> {
    Ok(Json(build_meter_list(
        &state.snapshot_rx.borrow(),
        &state.store,
    )?))
}

async fn create_meter(
    State(state): State<WebState>,
    Json(body): Json<MeterRequest>,
) -> Result<(StatusCode, Json<Accepted>), ApiError> {
    let mut input = meter_input(body);
    input.upload_enabled = false;
    validate_local_meter(&state, None, &input)?;
    send_command(&state, UserCommand::SaveMeter { id: None, input }).await?;
    Ok((StatusCode::ACCEPTED, Json(Accepted { accepted: true })))
}

async fn update_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<MeterRequest>,
) -> ApiResult<Accepted> {
    let mut input = meter_input(body);
    let existing = state
        .store
        .meters()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .find(|meter| meter.id == id)
        .ok_or_else(|| ApiError::bad_request("设备不存在"))?;
    if existing.config_source == "PLATFORM" {
        return Err(ApiError::bad_request(
            "线上设备以平台为准，请在平台修改后重新同步",
        ));
    }
    input.upload_enabled = false;
    validate_local_meter(&state, Some(id), &input)?;
    send_command(
        &state,
        UserCommand::SaveMeter {
            id: Some(id),
            input,
        },
    )
    .await?;
    Ok(Json(Accepted { accepted: true }))
}

fn validate_local_meter(
    state: &WebState,
    editing_id: Option<i64>,
    input: &gw_store::MeterInput,
) -> Result<(), ApiError> {
    if !(1..=247).contains(&input.modbus_addr) {
        return Err(ApiError::bad_request("Modbus 地址必须在 1-247 之间"));
    }
    let channel = state
        .store
        .channels()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .find(|channel| channel.id == input.channel_id)
        .ok_or_else(|| ApiError::bad_request("请选择已存在的 RS485 通道"))?;
    if input.enabled && !channel.enabled {
        return Err(ApiError::bad_request("该 RS485 通道已停用，不能启用设备"));
    }
    if input.enabled {
        let conflict = state
            .store
            .meters()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .find(|meter| {
                meter.enabled
                    && meter.channel_id == input.channel_id
                    && meter.modbus_addr == input.modbus_addr
                    && editing_id != Some(meter.id)
            });
        if let Some(meter) = conflict {
            return Err(ApiError::bad_request(format!(
                "同一 RS485 通道内 Modbus 地址不能重复：{} 地址 {} 已被 {} 使用",
                input.channel_id, input.modbus_addr, meter.device_sn
            )));
        }
    }
    if state
        .store
        .register_map(&input.profile)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_empty()
    {
        return Err(ApiError::bad_request("请选择已配置测点的物模型"));
    }
    Ok(())
}

async fn delete_meter(Path(id): Path<i64>, State(state): State<WebState>) -> ApiResult<Accepted> {
    send_command(&state, UserCommand::DeleteMeter { id }).await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn bind_meter(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<BindMeterRequest>,
) -> ApiResult<Accepted> {
    state
        .store
        .bind_meter_to_channel(id, &body.channel_id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn read_meter(Path(id): Path<i64>, State(state): State<WebState>) -> ApiResult<Accepted> {
    send_command(&state, UserCommand::ReadNow { meter_id: id }).await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn set_continuous_pull(
    Path(id): Path<i64>,
    State(state): State<WebState>,
    Json(body): Json<ContinuousPullRequest>,
) -> ApiResult<Accepted> {
    send_command(
        &state,
        UserCommand::SetContinuousPull {
            meter_id: id,
            enabled: body.enabled,
        },
    )
    .await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn collection_history(
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
    Ok(Json(CollectHistoryPageDto {
        page_num,
        page_size,
        total,
        rows: result,
    }))
}

async fn channels(State(state): State<WebState>) -> ApiResult<Vec<ChannelDto>> {
    Ok(Json(
        state
            .store
            .channels()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .map(|c| ChannelDto {
                id: c.id,
                name: c.name,
                protocol: c.protocol,
                port: c.port,
                baud: c.baud,
                data_bits: c.data_bits,
                stop_bits: c.stop_bits,
                parity: c.parity,
                timeout_ms: c.timeout_ms,
                retry_count: c.retry_count,
                poll_interval_seconds: c.poll_interval_s,
                config_source: c.config_source,
                enabled: c.enabled,
            })
            .collect(),
    ))
}

async fn create_channel(
    State(state): State<WebState>,
    Json(body): Json<ChannelRequest>,
) -> ApiResult<ChannelDto> {
    let id = next_channel_id(&state)?;
    let channel = channel_record(id, body)?;
    state
        .store
        .upsert_channel(&channel, now_ms())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    refresh_channel_transport(&state, &channel).await?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(channel_dto(channel)))
}

async fn save_channel(
    Path(id): Path<String>,
    State(state): State<WebState>,
    Json(body): Json<ChannelRequest>,
) -> ApiResult<Accepted> {
    let channel = channel_record(id, body)?;
    state
        .store
        .upsert_channel(&channel, now_ms())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    refresh_channel_transport(&state, &channel).await?;
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn delete_channel(
    Path(id): Path<String>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(ApiError::bad_request("请选择要删除的 RS485 通道"));
    }
    state
        .store
        .delete_channel_with_meters(&id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    state.transports.remove(&id);
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    Ok(Json(Accepted { accepted: true }))
}

fn next_channel_id(state: &WebState) -> Result<String, ApiError> {
    let channels = state
        .store
        .channels()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut next = 1;
    loop {
        let id = format!("rs485-{next}");
        if channels.iter().all(|channel| channel.id != id) {
            return Ok(id);
        }
        next += 1;
    }
}

fn channel_record(
    id: String,
    body: ChannelRequest,
) -> Result<gw_store::Rs485ChannelRecord, ApiError> {
    let id = id.trim().to_string();
    let port = body.port.trim().to_string();
    if id.is_empty() || port.is_empty() || body.baud == 0 {
        return Err(ApiError::bad_request("通道编号、串口和波特率不能为空"));
    }
    if !(5..=8).contains(&body.data_bits) {
        return Err(ApiError::bad_request("数据位必须在 5-8 之间"));
    }
    if !(1..=2).contains(&body.stop_bits) {
        return Err(ApiError::bad_request("停止位必须是 1 或 2"));
    }
    let parity = body.parity.trim().to_ascii_uppercase();
    if !matches!(parity.as_str(), "NONE" | "EVEN" | "ODD") {
        return Err(ApiError::bad_request("校验位必须是 NONE、EVEN 或 ODD"));
    }
    if !(100..=60_000).contains(&body.timeout_ms) || body.retry_count > 10
        || !(5..=86_400).contains(&body.poll_interval_seconds)
    {
        return Err(ApiError::bad_request("超时、重试或默认采集周期超出允许范围"));
    }
    Ok(gw_store::Rs485ChannelRecord {
        id,
        name: if body.name.trim().is_empty() {
            "RS485".into()
        } else {
            body.name.trim().to_string()
        },
        protocol: "MODBUS_RTU".into(),
        port,
        baud: body.baud,
        data_bits: body.data_bits,
        stop_bits: body.stop_bits,
        parity,
        timeout_ms: body.timeout_ms,
        retry_count: body.retry_count,
        poll_interval_s: body.poll_interval_seconds,
        config_source: "LOCAL".into(),
        enabled: body.enabled,
    })
}

fn channel_dto(channel: gw_store::Rs485ChannelRecord) -> ChannelDto {
    ChannelDto {
        id: channel.id,
        name: channel.name,
        protocol: channel.protocol,
        port: channel.port,
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity,
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
        poll_interval_seconds: channel.poll_interval_s,
        config_source: channel.config_source,
        enabled: channel.enabled,
    }
}

async fn refresh_channel_transport(
    state: &WebState,
    channel: &gw_store::Rs485ChannelRecord,
) -> Result<(), ApiError> {
    state.transports.remove(&channel.id);
    if !channel.enabled {
        return Ok(());
    }
    if channel.protocol != "MODBUS_RTU" {
        return Err(ApiError::bad_request(format!("暂不支持通道协议 {}", channel.protocol)));
    }
    let serial = gw_collector::SerialConfig {
        port: channel.port.clone(),
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity.clone(),
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
    };
    match gw_collector::ModbusRtuTransport::connect(&serial).await {
        Ok(transport) => {
            state.transports.insert(
                channel.id.clone(),
                Arc::new(tokio::sync::Mutex::new(transport)),
            );
            Ok(())
        }
        Err(error) => Err(ApiError::unavailable(format!(
            "通道 {} 无法打开串口 {}: {error}", channel.id, channel.port
        ))),
    }
}

async fn rebuild_channel_transports(state: &WebState) -> Vec<(String, Result<(), String>)> {
    for channel_id in state.transports.channel_ids() {
        state.transports.remove(&channel_id);
    }
    let channels = match state.store.channels() {
        Ok(channels) => channels,
        Err(error) => return vec![("__store__".into(), Err(error.to_string()))],
    };
    let mut results = Vec::new();
    for channel in channels.into_iter().filter(|channel| channel.enabled) {
        let channel_id = channel.id.clone();
        let result = refresh_channel_transport(state, &channel)
            .await
            .map_err(|error| error.message);
        results.push((channel_id, result));
    }
    results
}

async fn thing_models(State(state): State<WebState>) -> ApiResult<Vec<ThingModelDto>> {
    let summaries = state
        .store
        .thing_models()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        summaries
            .into_iter()
            .map(|model| {
                let points = state
                    .store
                    .register_map(&model.profile)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| ThingPointDto {
                        code: p.point_code,
                        name: p.point_name,
                        unit: p.unit,
                        function_code: p.func,
                        address: p.address,
                        quantity: p.quantity,
                        field_offset: p.field_offset,
                        field_quantity: p.field_quantity,
                        bit_offset: p.bit_offset,
                        bit_length: p.bit_length,
                        data_type: p.data_type,
                        byte_order: p.byte_order,
                        scale: p.scale,
                        offset: p.offset,
                    })
                    .collect();
                ThingModelDto {
                    profile: model.profile,
                    name: model.name,
                    version: model.version,
                    source: model.source,
                    point_count: model.point_count,
                    device_count: model.device_count,
                    points,
                }
            })
            .collect(),
    ))
}

async fn delete_thing_model(
    Path(profile): Path<String>,
    State(state): State<WebState>,
) -> ApiResult<Accepted> {
    state
        .store
        .delete_thing_model(&profile)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(Accepted { accepted: true }))
}

async fn network_snapshot(State(state): State<WebState>) -> ApiResult<WifiDto> {
    let (value, _) = cached_network(&state).await?;
    Ok(Json(wifi_dto(value)))
}

async fn network_scan(State(state): State<WebState>) -> ApiResult<Vec<WifiNetworkDto>> {
    let manager = state.network.clone();
    let value = tokio::task::spawn_blocking(move || manager.scan())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(value.into_iter().map(network_dto).collect()))
}

async fn network_connect(
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

async fn network_disconnect(State(state): State<WebState>) -> ApiResult<Accepted> {
    let manager = state.network.clone();
    tokio::task::spawn_blocking(move || manager.disconnect())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    invalidate_network_cache(&state).await;
    Ok(Json(Accepted { accepted: true }))
}

async fn network_forget(
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

async fn platform_config(State(state): State<WebState>) -> ApiResult<PlatformConfig> {
    let get = |key: &str, fallback: String| {
        state
            .store
            .get_config(key)
            .ok()
            .flatten()
            .unwrap_or(fallback)
    };
    let password = get("mqtt_password", state.bootstrap.cloud.mqtt_password.clone());
    Ok(Json(PlatformConfig {
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
        mqtt_username: get("mqtt_username", state.bootstrap.cloud.mqtt_username.clone()),
        password_configured: !password.is_empty(),
        heartbeat_s: get("heartbeat_s", state.bootstrap.cloud.heartbeat_s.to_string())
            .parse()
            .unwrap_or(state.bootstrap.cloud.heartbeat_s),
    }))
}

async fn save_platform(
    State(state): State<WebState>,
    Json(body): Json<PlatformRequest>,
) -> ApiResult<Accepted> {
    if body.gateway_id.trim().is_empty()
        || body.gateway_sn.trim().is_empty()
        || body.mqtt_host.trim().is_empty()
    {
        return Err(ApiError::bad_request("网关标识和 MQTT 地址不能为空"));
    }
    let gateway_id = body.gateway_id.trim().to_string();
    let gateway_sn = body.gateway_sn.trim().to_string();
    let platform_http_url = if body.platform_http_url.trim().is_empty() {
        state.bootstrap.cloud.platform_http_url.clone()
    } else {
        body.platform_http_url
            .trim()
            .trim_end_matches('/')
            .to_string()
    };
    let mqtt_host = body.mqtt_host.trim().to_string();
    let mqtt_username = body.mqtt_username.trim().to_string();
    let password = if body.mqtt_password.is_empty() {
        state
            .store
            .get_config("mqtt_password")
            .ok()
            .flatten()
            .unwrap_or_default()
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
        send_command(&state, UserCommand::SetHeartbeat { seconds }).await?;
    }
    Ok(Json(Accepted { accepted: true }))
}

async fn diagnose_platform(State(state): State<WebState>) -> ApiResult<Accepted> {
    send_command(&state, UserCommand::DiagnoseCloud).await?;
    Ok(Json(Accepted { accepted: true }))
}

async fn sync_platform_config(State(state): State<WebState>) -> ApiResult<SyncResult> {
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
        return Err(ApiError::bad_request("未配置网关密钥，不能执行线上同步"));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let response = client
        .get(format!(
            "{}/api/platform/edge/config",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .send()
        .await
        .map_err(|e| ApiError::unavailable(format!("平台配置同步连接失败: {e}")))?;
    let status = response.status();
    let envelope: PlatformEnvelope<SyncPayload> = response
        .json()
        .await
        .map_err(|e| ApiError::unavailable(format!("平台配置响应无法解析: {e}")))?;
    if !status.is_success() || envelope.code != 0 {
        return Err(ApiError::unavailable(format!(
            "平台拒绝同步: {}",
            envelope.message
        )));
    }
    let payload = envelope
        .data
        .ok_or_else(|| ApiError::unavailable("平台未返回配置数据"))?;
    if payload.models.iter().any(|model| model.read_blocks.is_empty() || model.fields.is_empty() || model.bindings.is_empty()) {
        let _ = state.store.record_sync_failure(
            Some(&payload.desired_revision),
            "物模型缺少现场采集点",
            now_ms(),
        );
        return Err(ApiError::bad_request(
            "平台物模型缺少 Modbus 采集点，已拒绝应用",
        ));
    }
    let channels = payload
        .channels
        .iter()
        .map(|channel| gw_store::SyncedChannel {
            channel_id: channel.channel_id.clone(),
            channel_name: channel.channel_name.clone(),
            protocol: channel.protocol.clone(),
            serial_port: channel.serial_port.clone(),
            baud_rate: channel.baud_rate,
            data_bits: channel.data_bits,
            stop_bits: channel.stop_bits,
            parity: channel.parity.clone(),
            timeout_ms: channel.timeout_ms,
            retry_count: channel.retry_count,
            poll_interval_s: channel.poll_interval_seconds,
            enabled: channel.enabled,
        })
        .collect::<Vec<_>>();
    let channel_precheck = validate_platform_channels(&channels);
    let channel_precheck_errors = channel_precheck
        .iter()
        .filter_map(|(channel, result)| {
            result
                .as_ref()
                .err()
                .map(|error| format!("{channel}: {error}"))
        })
        .collect::<Vec<_>>();
    if !channel_precheck_errors.is_empty() {
        let message = channel_precheck_errors.join("; ");
        let _ = state
            .store
            .record_sync_failure(Some(&payload.desired_revision), &message, now_ms());
        let _ = client
            .post(format!(
                "{}/api/platform/edge/config/ack",
                base.trim_end_matches('/')
            ))
            .header("X-Gateway-Sn", &gateway_sn)
            .header("X-Gateway-Secret", &secret)
            .json(&serde_json::json!({
                "appliedRevision": payload.desired_revision,
                "configChecksum": payload.config_checksum,
                "status": "FAILED",
                "error": message,
                "portInventory": serial_port_inventory(),
                "resources": sync_ack_resources(&payload, &channel_precheck)
            }))
            .send()
            .await;
        return Err(ApiError::unavailable(format!(
            "平台配置未应用，已保留本地配置: {message}"
        )));
    }
    let models = payload
        .models
        .iter()
        .map(|model| gw_store::SyncedThingModel {
            profile: model.profile_key.clone(),
            name: model.model_name.clone(),
            version: model.version.clone(),
            platform_model_id: model.model_version_id,
            points: model
                .bindings
                .iter()
                .filter_map(|binding| {
                    let field = model.fields.iter().find(|field| field.id == binding.protocol_field_id)?;
                    let block = model.read_blocks.iter().find(|block| block.id == field.read_block_id)?;
                    Some(gw_store::RegisterMapRow {
                        profile: model.profile_key.clone(),
                        point_code: normalize_point_code(&binding.point_code),
                        point_name: field.field_name.clone(),
                        unit: String::new(),
                        func: block.function_code,
                        address: block.start_address,
                        quantity: block.register_count,
                        field_offset: field.register_offset,
                        field_quantity: field.register_length,
                        bit_offset: field.bit_offset,
                        bit_length: field.bit_length,
                        data_type: field.value_type.to_ascii_lowercase(),
                        byte_order: field.byte_order.trim().to_ascii_uppercase(),
                        scale: field.decode_factor * binding.canonical_factor,
                        offset: field.decode_offset * binding.canonical_factor + binding.canonical_offset,
                    })
                })
                .collect(),
            commands: model.commands.iter().map(|command| gw_store::ProtocolCommandRow {
                profile: model.profile_key.clone(),
                command_code: command.command_code.trim().to_ascii_uppercase(),
                command_name: command.command_name.clone(),
                function_code: command.function_code,
                register_address: command.register_address,
                encode_type: command.encode_type.trim().to_ascii_uppercase(),
                fixed_value: command.fixed_value,
                parameter_json: command.parameter_json.as_ref().map(ToString::to_string),
            }).collect(),
        })
        .collect::<Vec<_>>();
    let devices = payload
        .devices
        .iter()
        .map(|device| gw_store::SyncedDevice {
            platform_device_id: device.platform_device_id,
            device_sn: device.device_sn.clone(),
            device_name: device.device_name.clone(),
            modbus_addr: device.modbus_addr,
            profile: device.profile_key.clone(),
            channel_id: device.channel_id.clone(),
            model_version: device.model_version.clone(),
            collect_interval_s: device.collect_interval_s,
            enabled: device.enabled,
        })
        .collect::<Vec<_>>();
    state
        .store
        .apply_platform_sync(&payload.desired_revision, &channels, &models, &devices, now_ms())
        .map_err(|e| {
            let message = format!("配置冲突或写入失败: {e}");
            let _ = state.store.record_sync_failure(
                Some(&payload.desired_revision),
                &message,
                now_ms(),
            );
            ApiError::bad_request(message)
        })?;
    let channel_results = rebuild_channel_transports(&state).await;
    let channel_errors = channel_results
        .iter()
        .filter_map(|(channel, result)| result.as_ref().err().map(|error| format!("{channel}: {error}")))
        .collect::<Vec<_>>();
    if !channel_errors.is_empty() {
        let message = channel_errors.join("; ");
        let _ = state.store.record_sync_failure(Some(&payload.desired_revision), &message, now_ms());
        let resources = sync_ack_resources(&payload, &channel_results);
        let _ = client
            .post(format!("{}/api/platform/edge/config/ack", base.trim_end_matches('/')))
            .header("X-Gateway-Sn", &gateway_sn)
            .header("X-Gateway-Secret", &secret)
            .json(&serde_json::json!({
                "appliedRevision": payload.desired_revision,
                "configChecksum": payload.config_checksum,
                "status": "FAILED",
                "error": message,
                "portInventory": serial_port_inventory(),
                "resources": resources
            }))
            .send()
            .await;
        return Err(ApiError::unavailable(format!("平台配置未应用: {message}")));
    }
    send_command(&state, UserCommand::ReloadConfiguration).await?;
    state.store
        .record_sync_applied(&payload.desired_revision, now_ms())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let ack = client
        .post(format!(
            "{}/api/platform/edge/config/ack",
            base.trim_end_matches('/')
        ))
        .header("X-Gateway-Sn", &gateway_sn)
        .header("X-Gateway-Secret", &secret)
        .json(&serde_json::json!({
            "appliedRevision": payload.desired_revision,
            "configChecksum": payload.config_checksum,
            "status": "APPLIED",
            "portInventory": serial_port_inventory(),
            "resources": sync_ack_resources(&payload, &channel_results)
        }))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false);
    Ok(Json(SyncResult {
        applied: true,
        revision: payload.desired_revision,
        channels: channels.len(),
        devices: devices.len(),
        models: models.len(),
        cloud_acknowledged: ack,
    }))
}

fn serial_port_inventory() -> Vec<serde_json::Value> {
    tokio_serial::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| serde_json::json!({
            "portKey": port.port_name,
            "systemPath": port.port_name,
            "portType": format!("{:?}", port.port_type),
            "available": true
        }))
        .collect()
}

fn validate_platform_channels(
    channels: &[gw_store::SyncedChannel],
) -> Vec<(String, Result<(), String>)> {
    channels
        .iter()
        .map(|channel| {
            let result = if !channel.enabled {
                Ok(())
            } else if channel.protocol.trim().to_ascii_uppercase() != "MODBUS_RTU" {
                Err(format!("暂不支持通道协议 {}", channel.protocol))
            } else {
                let port = channel.serial_port.trim();
                if port.is_empty() {
                    Err("平台未下发串口路径".to_string())
                } else if !FsPath::new(port).exists() {
                    let inventory = serial_port_inventory()
                        .into_iter()
                        .filter_map(|item| {
                            item.get("systemPath")
                                .and_then(|value| value.as_str())
                                .map(str::to_string)
                        })
                        .collect::<Vec<_>>();
                    let hint = if inventory.is_empty() {
                        "当前系统未枚举到可用串口".to_string()
                    } else {
                        format!("当前可用串口: {}", inventory.join(", "))
                    };
                    Err(format!("平台下发串口 {port} 不存在，{hint}"))
                } else {
                    Ok(())
                }
            };
            (channel.channel_id.clone(), result)
        })
        .collect()
}

fn sync_ack_resources(
    payload: &SyncPayload,
    channel_results: &[(String, Result<(), String>)],
) -> Vec<serde_json::Value> {
    let mut resources = Vec::new();
    for channel in &payload.channels {
        let result = channel_results.iter().find(|(id, _)| id == &channel.channel_id);
        let error = result.and_then(|(_, value)| value.as_ref().err()).cloned();
        resources.push(serde_json::json!({
            "resourceType": "CHANNEL",
            "resourceKey": channel.channel_id,
            "status": if error.is_some() { "FAILED" } else { "APPLIED" },
            "message": error.unwrap_or_else(|| format!(
                "{} {} {}{}{}", channel.serial_port, channel.baud_rate,
                channel.data_bits, channel.parity, channel.stop_bits
            ))
        }));
    }
    for device in &payload.devices {
        resources.push(serde_json::json!({
            "resourceType": "DEVICE",
            "resourceKey": device.device_sn,
            "status": "APPLIED",
            "message": format!("{} 地址 {}", device.channel_id, device.modbus_addr)
        }));
    }
    for model in &payload.models {
        resources.push(serde_json::json!({
            "resourceType": "MODEL",
            "resourceKey": format!("{}:{}", model.profile_key, model.version),
            "status": "APPLIED",
            "message": format!("{} 个读块 / {} 个字段 / {} 个测点", model.read_blocks.len(), model.fields.len(), model.bindings.len())
        }));
        for binding in &model.bindings {
            let field = model.fields.iter().find(|item| item.id == binding.protocol_field_id);
            let block = field.and_then(|item| model.read_blocks.iter().find(|block| block.id == item.read_block_id));
            resources.push(serde_json::json!({
                "resourceType": "POINT",
                "resourceKey": format!("{}:{}:{}", model.profile_key, model.version, binding.point_code),
                "status": "APPLIED",
                "message": match (field, block) {
                    (Some(field), Some(block)) => format!("FC{} @{}+{} x{} {}", block.function_code, block.start_address, field.register_offset, field.register_length, field.byte_order),
                    _ => "绑定引用缺失".to_string()
                }
            }));
        }
    }
    resources
}

async fn send_command(state: &WebState, command: UserCommand) -> Result<(), ApiError> {
    state
        .command_tx
        .send(command)
        .await
        .map_err(|_| ApiError::unavailable("Agent 命令通道已关闭"))
}

fn meter_input(body: MeterRequest) -> MeterInput {
    MeterInput {
        device_sn: body.device_sn,
        device_name: body.device_name,
        modbus_addr: body.modbus_addr,
        profile: body.profile,
        channel_id: body.channel_id,
        upload_enabled: body.upload_enabled,
        collect_interval_s: body.collect_interval_s,
        enabled: body.enabled,
    }
}

async fn build_snapshot(state: &WebState) -> Result<GatewaySnapshotDto, ApiError> {
    let current = state.snapshot_rx.borrow().clone();
    let (net, scanned) = cached_network(state).await?;
    let get = |key: &str, fallback: String| {
        state
            .store
            .get_config(key)
            .ok()
            .flatten()
            .unwrap_or(fallback)
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

async fn refresh_network(data: &mut GatewaySnapshotDto, state: &WebState) {
    if let Ok((net, networks)) = cached_network(state).await {
        data.wifi = wifi_dto(net);
        data.networks = networks.into_iter().map(network_dto).collect();
    }
}

async fn refresh_slow_fields(data: &mut GatewaySnapshotDto, state: &WebState) {
    refresh_network(data, state).await;
    data.events = load_events(&state.store, 100).unwrap_or_default();
    data.uploads = load_uploads(&state.store, 100).unwrap_or_default();
    data.commands = load_commands(&state.store, 100).unwrap_or_default();
}

async fn cached_network(state: &WebState) -> Result<(NetworkSnapshot, Vec<WifiNetwork>), ApiError> {
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
    let network =
        tokio::task::spawn_blocking(move || (manager.snapshot(), manager.cached_networks()))
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

async fn invalidate_network_cache(state: &WebState) {
    let mut cache = state.network_cache.lock().await;
    cache.refreshed_at = None;
}

fn refresh_dynamic(
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
    data.meters = build_meter_list(current, store).unwrap_or_else(|_| {
        current
            .meters
            .iter()
            .map(|meter| meter_dto(meter, store))
            .collect()
    });
}

fn link_label(current: &Snapshot, _runtime: RuntimeDto) -> String {
    format!("MQTT {}", current.cloud.label())
}

fn load_events(store: &Store, limit: u32) -> Result<Vec<EventDto>, ApiError> {
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
        .chain(events.into_iter().enumerate().map(|(index, e)| {
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
        }))
        .collect();
    combined.sort_by_key(|(timestamp, _)| std::cmp::Reverse(*timestamp));
    combined.truncate(limit as usize);
    Ok(combined.into_iter().map(|(_, event)| event).collect())
}

fn load_commands(store: &Store, limit: u32) -> Result<Vec<CommandDto>, ApiError> {
    Ok(store
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
        .collect())
}

fn load_uploads(store: &Store, limit: u32) -> Result<Vec<UploadDto>, ApiError> {
    let rows = store
        .recent_outbox(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap_or_default();
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
                data_status: upload_data_status(&row.status, row.last_error.as_deref()).into(),
                latency: 0,
            }
        })
        .collect())
}

fn upload_access_status(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "sent" => "FORWARDED",
        "pending" => "PENDING",
        "deferred" => "DEFERRED",
        _ => "FAILED",
    }
}

fn upload_data_status(status: &str, error: Option<&str>) -> String {
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

fn build_meter_list(current: &Snapshot, store: &Store) -> Result<Vec<MeterDto>, ApiError> {
    let records = store
        .meters()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut dtos = current
        .meters
        .iter()
        .map(|meter| meter_dto(meter, store))
        .collect::<Vec<_>>();
    for record in records {
        if dtos.iter().any(|meter| meter.sn == record.device_sn) {
            continue;
        }
        dtos.push(meter_dto_from_record(&record, store));
    }
    dtos.sort_by(|a, b| {
        let a_staged = a.channel_id == "__staging__";
        let b_staged = b.channel_id == "__staging__";
        a_staged
            .cmp(&b_staged)
            .then_with(|| a.channel_id.cmp(&b.channel_id))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(dtos)
}

fn meter_dto(m: &MeterSnapshot, store: &Store) -> MeterDto {
    let point = |key: &str| {
        m.points
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
            .unwrap_or(0.0)
    };
    let record = store.meter_by_sn(&m.device_sn).ok().flatten();
    let mappings = store.register_map(&m.profile).unwrap_or_default();
    let mut points = mappings
        .iter()
        .map(|row| {
            let value = m
                .points
                .iter()
                .find(|(code, _)| code == &row.point_code)
                .map(|(_, value)| *value);
            PointDto {
                code: row.point_code.clone(),
                name: if row.point_name.is_empty() {
                    row.point_code.clone()
                } else {
                    row.point_name.clone()
                },
                value,
                unit: if row.unit.is_empty() {
                    default_unit(&row.point_code).to_string()
                } else {
                    row.unit.clone()
                },
                quality: if value.is_some() { m.quality } else { 0 },
                collect_time: if value.is_some() { m.last_read_ms } else { 0 },
            }
        })
        .collect::<Vec<_>>();
    for (code, value) in &m.points {
        if points.iter().any(|point| point.code == *code) {
            continue;
        }
        points.push(PointDto {
            code: code.clone(),
            name: code.clone(),
            value: Some(*value),
            unit: default_unit(code).to_string(),
            quality: m.quality,
            collect_time: m.last_read_ms,
        });
    }
    MeterDto {
        id: m.id,
        sn: m.device_sn.clone(),
        name: record
            .as_ref()
            .map(|r| r.device_name.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| m.device_sn.clone()),
        profile: m.profile.clone(),
        model_version: record
            .as_ref()
            .map(|r| r.model_version.clone())
            .unwrap_or_default(),
        channel_id: record
            .as_ref()
            .map(|r| r.channel_id.clone())
            .unwrap_or_else(default_channel_id),
        config_source: record
            .as_ref()
            .map(|r| r.config_source.clone())
            .unwrap_or_else(|| "LOCAL".into()),
        upload_enabled: record.as_ref().map(|r| r.upload_enabled).unwrap_or(false),
        enabled: record.as_ref().map(|r| r.enabled).unwrap_or(false),
        collect_interval_s: record.as_ref().map(|r| r.collect_interval_s).unwrap_or(0),
        continuous_pull: m.continuous_pull,
        address: m.addr,
        online: m.online,
        voltage: point("voltage_a"),
        current: point("current_a"),
        power: point("active_power_total"),
        energy: point("forward_active_energy"),
        last_read: format_time(m.last_read_ms),
        points,
    }
}

fn meter_dto_from_record(record: &MeterRecord, store: &Store) -> MeterDto {
    let mappings = store.register_map(&record.profile).unwrap_or_default();
    let points = mappings
        .into_iter()
        .map(|row| PointDto {
            code: row.point_code.clone(),
            name: if row.point_name.is_empty() {
                row.point_code.clone()
            } else {
                row.point_name
            },
            value: None,
            unit: if row.unit.is_empty() {
                default_unit(&row.point_code).to_string()
            } else {
                row.unit
            },
            quality: 0,
            collect_time: 0,
        })
        .collect();
    MeterDto {
        id: record.id,
        sn: record.device_sn.clone(),
        name: if record.device_name.is_empty() {
            record.device_sn.clone()
        } else {
            record.device_name.clone()
        },
        profile: record.profile.clone(),
        model_version: record.model_version.clone(),
        channel_id: record.channel_id.clone(),
        config_source: record.config_source.clone(),
        upload_enabled: record.upload_enabled,
        enabled: record.enabled,
        collect_interval_s: record.collect_interval_s,
        continuous_pull: false,
        address: record.modbus_addr,
        online: false,
        voltage: 0.0,
        current: 0.0,
        power: 0.0,
        energy: 0.0,
        last_read: "--".into(),
        points,
    }
}

fn load_alarm_rules(state: &WebState) -> Result<AlarmRuleConfigDto, ApiError> {
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

fn alarm_rule_record(
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
        return Err(ApiError::bad_request(
            "告警比较符只支持 >、>=、<、<=、==、!=",
        ));
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

fn alarm_rule_dto(row: AlarmRuleRecord) -> AlarmRuleDto {
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

fn collect_sample_dto(
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
            points.push(CollectPointDto {
                unit: default_unit(&code).into(),
                name: code.clone(),
                code,
                value,
            });
        }
    } else {
        for mapping in mappings {
            if let Some(value) = raw_points.get(&mapping.point_code) {
                points.push(CollectPointDto {
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
                point.code.as_str(),
                "active_power_total"
                    | "forward_active_energy"
                    | "voltage_a"
                    | "current_a"
                    | "frequency"
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

fn default_unit(code: &str) -> &'static str {
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

fn normalize_point_code(code: &str) -> String {
    code.trim().to_ascii_lowercase()
}

fn quality_label(quality: u32) -> &'static str {
    match quality {
        0 => "有效",
        1 => "可疑",
        2 => "缺失",
        3 => "异常",
        _ => "待确认",
    }
}

fn quality_status(quality: u32) -> &'static str {
    match quality {
        0 => "ok",
        1 => "warn",
        2 | 3 => "error",
        _ => "warn",
    }
}

fn wifi_dto(n: NetworkSnapshot) -> WifiDto {
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
fn network_dto(n: WifiNetwork) -> WifiNetworkDto {
    WifiNetworkDto {
        ssid: n.ssid,
        signal: n.signal,
        security: n.security,
        connected: n.connected,
    }
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
fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}天 {hours:02}:{minutes:02}")
    } else {
        format!("{hours:02}:{minutes:02}")
    }
}

fn item(key: impl Into<String>, value: impl Into<String>) -> SystemInfoItemDto {
    SystemInfoItemDto {
        key: key.into(),
        value: value.into(),
    }
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn read_os_release() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

async fn read_cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    for key in ["Hardware", "model name", "Processor"] {
        if let Some(line) = text.lines().find(|line| line.starts_with(key)) {
            return line
                .split_once(':')
                .map(|(_, value)| value.trim().to_string());
        }
    }
    if let Some(model) = read_trim("/proc/device-tree/model") {
        return Some(model.trim_end_matches('\0').to_string());
    }
    let lscpu = command_output("lscpu", &[]).await;
    lscpu.lines().find_map(|line| {
        line.strip_prefix("Model name:")
            .or_else(|| line.strip_prefix("Architecture:"))
            .map(|value| value.trim().to_string())
    })
}

fn read_memory_info() -> Vec<SystemInfoItemDto> {
    let mut values = BTreeMap::new();
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    [
        "MemTotal",
        "MemAvailable",
        "MemFree",
        "Buffers",
        "Cached",
        "SwapTotal",
        "SwapFree",
    ]
    .into_iter()
    .map(|key| item(key, values.get(key).cloned().unwrap_or_else(|| "--".into())))
    .collect()
}

fn read_thermal_zones() -> Vec<SystemInfoItemDto> {
    let mut zones = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let label = fs::read_to_string(path.join("type"))
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| name.to_string());
            let value = fs::read_to_string(path.join("temp"))
                .ok()
                .and_then(|text| text.trim().parse::<f64>().ok())
                .map(|raw| {
                    let celsius = if raw > 1000.0 { raw / 1000.0 } else { raw };
                    format!("{celsius:.1} °C")
                })
                .unwrap_or_else(|| "--".into());
            zones.push(item(label, value));
        }
    }
    zones.sort_by(|a, b| a.key.cmp(&b.key));
    zones
}

async fn read_storage_info() -> Vec<SystemInfoItemDto> {
    let output = command_output("df", &["-h", "/", "/usr/share/park-gateway", "/tmp"]).await;
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 6 {
                return None;
            }
            Some(item(
                parts[5],
                format!(
                    "{} 已用 {} / 可用 {} / 使用率 {}",
                    parts[1], parts[2], parts[3], parts[4]
                ),
            ))
        })
        .collect()
}

async fn read_kernel_params() -> Vec<SystemInfoItemDto> {
    let keys = [
        "kernel.ostype",
        "kernel.osrelease",
        "kernel.version",
        "kernel.hostname",
        "vm.swappiness",
        "vm.dirty_ratio",
        "vm.dirty_background_ratio",
        "fs.file-max",
        "fs.inotify.max_user_watches",
        "net.ipv4.ip_forward",
        "net.core.somaxconn",
        "net.ipv4.tcp_keepalive_time",
    ];
    let mut rows = Vec::new();
    for key in keys {
        rows.push(item(key, command_output("sysctl", &["-n", key]).await));
    }
    rows
}

async fn command_output(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() { "--".into() } else { text }
        }
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if text.is_empty() { "--".into() } else { text }
        }
        Err(_) => "--".into(),
    }
}
