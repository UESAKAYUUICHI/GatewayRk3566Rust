//! Agent 装配与任务编排。
//!
//! 任务拓扑：
//! ```text
//! poller(每表) ──▶ meter_sample(SQLite) ──5分钟归批──▶ outbox ──▶ CloudLink(MQTT QoS1)
//!                     ▲ 每个中间样本均持久化，归批与 outbox 关联在同一事务完成
//! heartbeat(30s，含子设备状态) ─────────────────────▶ CloudLink
//! link 下行指令 ──mpsc──▶ commander ──执行+回执──▶ CloudLink / command_log
//! 全部任务 ──▶ AppState ──watch(1Hz)──▶ Snapshot（UI/headless 观察者）
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gw_collector::{MeterTransport, ModbusRtuTransport, SerialConfig};
use gw_collector::profile::{DataType, MeterProfile, RegisterSpec};
use gw_core::clock::{ClockAssessor, ClockTrust};
use gw_core::command::{self, GatewayAction};
use gw_core::energy::{self, EnergyVerdict};
use gw_core::policy;
use gw_core::snapshot::{EventLine, MeterSnapshot, Snapshot};
use gw_link::port::{CloudLink, RawCommand};
use gw_proto::points::FORWARD_ACTIVE_ENERGY;
use gw_proto::{self as proto, topics};
use gw_store::{MeterInput, MeterRecord, Store};
use tokio::sync::{Mutex, Notify, broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::BootstrapConfig;
use crate::state::{AppState, MeterRuntime, PollerRegistry, SharedState};
use crate::transport_registry::TransportRegistry;

/// 时钟源端口。
pub trait ClockSource: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// 真实时钟。
pub struct RealClock;

impl ClockSource for RealClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 重启钩子端口。
pub trait RebootHook: Send + Sync {
    fn reboot(&self);
}

/// 生产重启动作：systemctl reboot（非阻塞发起）。
pub struct StdReboot;

impl RebootHook for StdReboot {
    fn reboot(&self) {
        tracing::warn!("REBOOT_GATEWAY 指令触发重启");
        let _ = std::process::Command::new("systemctl")
            .arg("reboot")
            .spawn();
    }
}

/// 空重启动作，用于非 Linux 环境避免误重启宿主机。
pub struct NoopReboot;

impl RebootHook for NoopReboot {
    fn reboot(&self) {}
}

/// agent 依赖集合（全部 trait/共享句柄）。
pub struct AgentDeps {
    pub store: Arc<Store>,
    pub link: Arc<dyn CloudLink>,
    pub transports: Arc<TransportRegistry>,
    pub clock: Arc<dyn ClockSource>,
    pub reboot: Arc<dyn RebootHook>,
    pub bootstrap: BootstrapConfig,
}

/// agent 对外句柄。
pub struct AgentHandle {
    pub snapshot_rx: watch::Receiver<Snapshot>,
    pub cmd_tx: mpsc::Sender<crate::ui_command::UserCommand>,
    pub events_rx: broadcast::Receiver<EventLine>,
    pub store: Arc<Store>,
    pub shutdown: CancellationToken,
}

/// 事件发射器：落库 + 广播给 UI。
#[derive(Clone)]
struct Emitter {
    store: Arc<Store>,
    clock: Arc<dyn ClockSource>,
    tx: broadcast::Sender<EventLine>,
}

impl Emitter {
    fn emit(&self, level: &str, source: &str, message: impl Into<String>) {
        let line = EventLine {
            ts_ms: self.clock.now_ms(),
            level: level.to_string(),
            source: source.to_string(),
            message: message.into(),
        };
        let _ = self.tx.send(line.clone());
        if let Err(e) = self.store.log_event(&gw_store::EventRow {
            ts_ms: line.ts_ms,
            level: line.level,
            source: line.source,
            message: line.message,
        }) {
            tracing::warn!("事件落库失败: {e}");
        }
    }
}

/// 启动 agent。command_rx 为云链路下行指令的消费端（由调用方在构造链路时接线）。
pub fn spawn_agent(
    deps: AgentDeps,
    command_rx: mpsc::Receiver<RawCommand>,
    shutdown: CancellationToken,
) -> AgentHandle {
    let AgentDeps {
        store,
        link,
        transports,
        clock,
        reboot,
        bootstrap,
    } = deps;

    // ---- 启动种子：档案导入 + 电表种子入库 + 运行时构建 ----
    let now = clock.now_ms();
    import_profiles(&store, &bootstrap);
    seed_meters(&store, &bootstrap, now);

    let meters: Vec<MeterRuntime> = store
        .meters()
        .unwrap_or_default()
        .into_iter()
        .map(|record| {
            let reading = store.reading(record.id).unwrap_or(None);
            MeterRuntime::from_record(record, reading)
        })
        .collect();

    let heartbeat_s = store
        .get_config("heartbeat_s")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(bootstrap.cloud.heartbeat_s);

    let state: SharedState = Arc::new(AppState::new(
        bootstrap.cloud.gateway_id.clone(),
        bootstrap.cloud.gateway_sn.clone(),
        heartbeat_s,
        meters,
        now,
    ));
    let registry = Arc::new(PollerRegistry::default());
    let assessor = ClockAssessor::default();
    let notify_publish = Arc::new(Notify::new());
    let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot {
        cloud: link.health(),
        clock_trusted: matches!(assessor.assess(now), ClockTrust::Trusted),
        outbox_pending: 0,
        outbox_sent_total: 0,
        meters_online: 0,
        meters_total: 0,
        today_kwh_total: 0.0,
        last_publish_ms: 0,
        last_heartbeat_ms: 0,
        started_ms: now,
        version: env!("CARGO_PKG_VERSION").to_string(),
        meters: Vec::new(),
    });
    let (events_tx, events_rx) = broadcast::channel(256);
    let emitter = Emitter {
        store: store.clone(),
        clock: clock.clone(),
        tx: events_tx.clone(),
    };
    let (cmd_tx, cmd_rx) = mpsc::channel(16);

    let ctx = TaskCtx {
        store: store.clone(),
        link: link.clone(),
        transports: transports.clone(),
        clock: clock.clone(),
        state: state.clone(),
        registry: registry.clone(),
        emitter: emitter.clone(),
        notify_publish: notify_publish.clone(),
        assessor,
        tz_offset_s: bootstrap.tz_offset_s,
        report_interval_s: bootstrap.cloud.report_interval_s,
    };

    emitter.emit(
        "INFO",
        "agent",
        format!("网关启动 v{}", env!("CARGO_PKG_VERSION")),
    );

    for meter in state.enabled_meters() {
        tokio::spawn(poller_task(ctx.clone(), meter.record.id, shutdown.clone()));
    }
    tokio::spawn(publisher_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(report_batch_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(heartbeat_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(commander_task(
        ctx.clone(),
        reboot,
        command_rx,
        shutdown.clone(),
    ));
    tokio::spawn(user_command_task(ctx.clone(), cmd_rx, shutdown.clone()));
    tokio::spawn(snapshotter_task(ctx, snapshot_tx, shutdown.clone()));
    tokio::spawn(maintainer_task(
        store.clone(),
        clock.clone(),
        shutdown.clone(),
    ));

    AgentHandle {
        snapshot_rx,
        cmd_tx,
        events_rx,
        store,
        shutdown,
    }
}

/// 各任务共享的上下文。
#[derive(Clone)]
struct TaskCtx {
    store: Arc<Store>,
    link: Arc<dyn CloudLink>,
    transports: Arc<TransportRegistry>,
    clock: Arc<dyn ClockSource>,
    state: SharedState,
    registry: Arc<PollerRegistry>,
    emitter: Emitter,
    notify_publish: Arc<Notify>,
    assessor: ClockAssessor,
    tz_offset_s: i32,
    report_interval_s: u64,
}

impl TaskCtx {
    fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    fn clock_trusted(&self) -> bool {
        matches!(self.assessor.assess(self.now_ms()), ClockTrust::Trusted)
    }

    /// 差异同步采集任务：新增的表启动任务，移除的表取消。
    fn sync_pollers(&self, shutdown: &CancellationToken) {
        let want: std::collections::HashSet<i64> = self
            .state
            .enabled_meters()
            .into_iter()
            .map(|m| m.record.id)
            .collect();
        for id in self.registry.current_ids() {
            if !want.contains(&id) {
                self.registry.cancel(id);
            }
        }
        for id in want {
            if !self.registry.current_ids().contains(&id) {
                self.registry.token_for(id); // 登记
                tokio::spawn(poller_task(self.clone(), id, shutdown.clone()));
            }
        }
    }
}

fn import_profiles(store: &Store, bootstrap: &BootstrapConfig) {
    let dir = std::path::Path::new(&bootstrap.profiles_dir);
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::info!("档案目录 {} 不存在，跳过导入", dir.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| MeterProfile::from_toml_str(&text).map_err(|e| e.to_string()))
        {
            Ok(profile) => {
                let rows: Vec<gw_store::RegisterMapRow> = profile
                    .register
                    .iter()
                    .map(|spec| gw_store::RegisterMapRow {
                        profile: profile.name.clone(),
                        point_code: spec.point.clone(),
                        point_name: local_point_metadata(&spec.point).0.to_string(),
                        unit: local_point_metadata(&spec.point).1.to_string(),
                        func: spec.func,
                        address: spec.address,
                        quantity: spec.quantity,
                        field_offset: 0,
                        field_quantity: spec.quantity,
                        bit_offset: None,
                        bit_length: None,
                        data_type: spec.data_type.as_str().to_string(),
                        byte_order: spec.byte_order.trim().to_ascii_uppercase(),
                        scale: spec.scale,
                        offset: spec.offset,
                    })
                    .collect();
                match store.replace_register_map(&profile.name, &rows) {
                    Ok(()) => {
                        let _ = store.register_local_thing_model(
                            &profile.name,
                            &profile.description,
                            0,
                        );
                        tracing::info!("已导入寄存器档案 {}（{} 点）", profile.name, rows.len())
                    }
                    Err(e) => tracing::warn!("档案 {} 入库失败: {e}", profile.name),
                }
            }
            Err(e) => tracing::warn!("跳过非法档案 {}: {e}", path.display()),
        }
    }
}

fn local_point_metadata(code: &str) -> (&'static str, &'static str) {
    match code {
        "voltage_a" => ("A相电压", "V"),
        "voltage_b" => ("B相电压", "V"),
        "voltage_c" => ("C相电压", "V"),
        "current_a" => ("A相电流", "A"),
        "current_b" => ("B相电流", "A"),
        "current_c" => ("C相电流", "A"),
        "active_power_total" => ("总有功功率", "kW"),
        "reactive_power_total" => ("总无功功率", "kvar"),
        "apparent_power_total" => ("总视在功率", "kVA"),
        "power_factor_total" => ("总功率因数", ""),
        "frequency" => ("频率", "Hz"),
        "forward_active_energy" => ("正向有功电能", "kWh"),
        _ => ("自定义测点", ""),
    }
}

fn seed_meters(store: &Store, bootstrap: &BootstrapConfig, now_ms: u64) {
    for seed in &bootstrap.meter {
        if store.meter_by_sn(&seed.device_sn).unwrap_or(None).is_some() {
            continue;
        }
        let input = MeterInput {
            device_sn: seed.device_sn.clone(),
            device_name: if seed.device_name.is_empty() {
                seed.device_sn.clone()
            } else {
                seed.device_name.clone()
            },
            modbus_addr: seed.modbus_addr,
            profile: seed.profile.clone(),
            channel_id: seed.channel_id.clone(),
            upload_enabled: seed.upload_enabled,
            collect_interval_s: seed.collect_interval_s,
            enabled: seed.enabled,
        };
        match store.insert_meter(&input, now_ms) {
            Ok(id) => tracing::info!("种子电表 {} 入库 id={id}", seed.device_sn),
            Err(e) => tracing::warn!("种子电表 {} 入库失败: {e}", seed.device_sn),
        }
    }
}

/// 档案行 → 解码规格。
fn specs_of(rows: &[gw_store::RegisterMapRow]) -> Vec<gw_store::RegisterMapRow> {
    rows.iter()
        .filter(|row| row.data_type.eq_ignore_ascii_case("boolean") || DataType::parse(&row.data_type).is_some())
        .cloned()
        .collect()
}

/// Read each block once, then split fields and map them to canonical point codes.
async fn poll_once<T: MeterTransport + ?Sized>(
    transport: &Mutex<T>,
    meter: &MeterRecord,
    specs: &[gw_store::RegisterMapRow],
) -> Result<Vec<(String, f64)>, gw_collector::TransportError> {
    let mut guard = transport.lock().await;
    let mut words: HashMap<(u8, u16, u16), Vec<u16>> = HashMap::new();
    for spec in specs {
        let key = (spec.func, spec.address, spec.quantity);
        if let std::collections::hash_map::Entry::Vacant(entry) = words.entry(key) {
            let got = guard.read_registers(spec.func, meter.modbus_addr, spec.address, spec.quantity).await?;
            entry.insert(got);
        }
    }
    let mut points: Vec<(String, f64)> = Vec::with_capacity(specs.len());
    for spec in specs {
        let Some(block) = words.get(&(spec.func, spec.address, spec.quantity)) else { continue };
        let start = spec.field_offset as usize;
        let end = start + spec.field_quantity as usize;
        if end > block.len() { continue; }
        let value = if let Some(bit_offset) = spec.bit_offset {
            let width = spec.bit_length.unwrap_or(1).min(16);
            let mask = if width == 16 { u16::MAX } else { (1u16 << width) - 1 };
            ((block[start] >> bit_offset) & mask) as f64 * spec.scale + spec.offset
        } else {
            let Some(data_type) = DataType::parse(&spec.data_type) else { continue };
            let field = RegisterSpec {
                point: spec.point_code.clone(), func: spec.func, address: 0,
                quantity: spec.field_quantity, data_type, byte_order: spec.byte_order.clone(),
                scale: spec.scale, offset: spec.offset,
            };
            MeterProfile::decode_one(&field, &block[start..end])
        };
        points.push((spec.point_code.clone(), value));
    }
    Ok(points)
}

/// 单表采集任务。周期每轮从状态重读（SET_INTERVAL 即时生效）；
/// 表删除/停用由 sync_pollers 取消令牌，本任务自然退出。
async fn poller_task(ctx: TaskCtx, meter_id: i64, shutdown: CancellationToken) {
    let token = ctx.registry.token_for(meter_id);
    loop {
        let configured_interval_s = ctx
            .state
            .meter_by_id(meter_id)
            .map(|m| m.record.collect_interval_s)
            .unwrap_or(0);
        let interval_s = if ctx.state.continuous_pull_enabled(meter_id) {
            5
        } else {
            configured_interval_s
        };
        if interval_s == 0 {
            return; // 表已删除或停用
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(interval_s.max(1))) => {}
        }
        let Some(meter) = ctx.state.meter_by_id(meter_id) else {
            return;
        };
        if !ctx
            .store
            .channel_enabled(&meter.record.channel_id)
            .unwrap_or(false)
        {
            let event_id = format!("CHANNEL_DISABLED:{}", meter.record.channel_id);
            raise_gateway_alarm(
                &ctx,
                &meter,
                &event_id,
                "CHANNEL_DISABLED",
                "MAJOR",
                format!("RS485 通道 {} 未启用或不存在", meter.record.channel_id),
            );
            continue;
        }
        recover_gateway_alarm(
            &ctx,
            &meter,
            &format!("CHANNEL_DISABLED:{}", meter.record.channel_id),
            "CHANNEL_DISABLED",
            format!("RS485 通道 {} 已恢复", meter.record.channel_id),
        );
        let rows = ctx
            .store
            .register_map(&meter.record.profile)
            .unwrap_or_default();
        if rows.is_empty() {
            ctx.emitter.emit(
                "WARN",
                "poller",
                format!(
                    "档案 {} 为空，跳过采集 {}",
                    meter.record.profile, meter.record.device_sn
                ),
            );
            continue;
        }
        let specs = specs_of(&rows);
        let Some(transport) = ensure_channel_transport(&ctx, &meter).await else {
            handle_channel_unavailable(&ctx, &meter);
            continue;
        };
        recover_gateway_alarm(
            &ctx,
            &meter,
            &format!("CHANNEL_UNAVAILABLE:{}", meter.record.channel_id),
            "CHANNEL_UNAVAILABLE",
            format!("RS485 通道 {} 传输已恢复", meter.record.channel_id),
        );
        match poll_once(&transport, &meter.record, &specs).await {
            Ok(points) => handle_sample(&ctx, &meter, points).await,
            Err(e) => handle_poll_failure(&ctx, &meter, e),
        }
    }
}

async fn ensure_channel_transport(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
) -> Option<crate::transport_registry::TransportHandle> {
    if let Some(transport) = ctx.transports.get(&meter.record.channel_id) {
        return Some(transport);
    }
    let channel = match ctx.store.channels() {
        Ok(channels) => channels
            .into_iter()
            .find(|channel| channel.id == meter.record.channel_id && channel.enabled),
        Err(error) => {
            ctx.emitter.emit(
                "WARN",
                "poller",
                format!("读取 RS485 通道配置失败: {error}"),
            );
            None
        }
    }?;
    let protocol = channel.protocol.trim().to_ascii_uppercase();
    if protocol != "MODBUS_RTU" {
        ctx.emitter.emit(
            "WARN",
            "poller",
            format!("通道 {} 协议 {protocol} 暂不支持", channel.id),
        );
        return None;
    }
    let serial = SerialConfig {
        port: channel.port.clone(),
        baud: channel.baud,
        data_bits: channel.data_bits,
        stop_bits: channel.stop_bits,
        parity: channel.parity.clone(),
        timeout_ms: channel.timeout_ms,
        retry_count: channel.retry_count,
    };
    match ModbusRtuTransport::connect(&serial).await {
        Ok(transport) => {
            let handle = Arc::new(Mutex::new(transport));
            ctx.transports.insert(channel.id.clone(), handle.clone());
            ctx.emitter.emit(
                "INFO",
                "poller",
                format!("RS485 通道 {} 已自动恢复: {}", channel.id, channel.port),
            );
            Some(handle)
        }
        Err(error) => {
            ctx.emitter.emit(
                "WARN",
                "poller",
                format!("RS485 通道 {} 自动恢复失败: {error}", channel.id),
            );
            None
        }
    }
}

fn handle_channel_unavailable(ctx: &TaskCtx, meter: &MeterRuntime) {
    raise_gateway_alarm(
        ctx,
        meter,
        &format!("CHANNEL_UNAVAILABLE:{}", meter.record.channel_id),
        "CHANNEL_UNAVAILABLE",
        "MAJOR",
        format!("RS485 通道 {} 未建立串口传输", meter.record.channel_id),
    );
}

/// 处理一次成功采样：能量账 + 最新快照 + 不可覆盖的历史样本。
async fn handle_sample(ctx: &TaskCtx, meter: &MeterRuntime, points: Vec<(String, f64)>) {
    let now = ctx.now_ms();
    recover_gateway_alarm(
        ctx,
        meter,
        &format!("DEVICE_OFFLINE:{}", meter.record.device_sn),
        "DEVICE_OFFLINE",
        format!("{} 采集已恢复", meter.record.device_sn),
    );
    let total = points
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(FORWARD_ACTIVE_ENERGY))
        .map(|(_, v)| *v);

    let verdict = energy::evaluate(meter.last_total_kwh, total.unwrap_or(f64::NAN));
    let reportable = match verdict {
        EnergyVerdict::Rollback { previous, current } => {
            ctx.emitter.emit(
                "WARN",
                "energy",
                format!(
                    "{} 读数回退 {previous:.2} → {current:.2} kWh（表清零/换表），样本不上报并重建基线",
                    meter.record.device_sn
                ),
            );
            false
        }
        EnergyVerdict::Delta(delta) => {
            if delta > 0.0 {
                let day = energy::day_key(now, ctx.tz_offset_s);
                if let Err(e) = ctx.store.add_energy(meter.record.id, &day, delta) {
                    tracing::warn!("电能日账写入失败: {e}");
                }
            }
            true
        }
        EnergyVerdict::First => true,
    };
    // NaN（本次无电能点位）按可上报处理，不参与能量账
    let reportable = reportable || total.is_none();

    ctx.state.update_meter(meter.record.id, |m| {
        m.online = true;
        m.consecutive_failures = 0;
        m.last_read_ms = now;
        m.last_quality = proto::model::QUALITY_NORMAL;
        m.last_points = points.clone();
        if let Some(t) = total {
            m.last_total_kwh = Some(t);
        }
    });
    let reading = gw_store::ReadingRecord {
        meter_id: meter.record.id,
        snapshot_json: serde_json::to_string(&points).unwrap_or_default(),
        read_ms: now,
        sample_interval_s: if ctx.state.continuous_pull_enabled(meter.record.id) {
            5
        } else {
            meter.record.collect_interval_s
        },
        quality: proto::model::QUALITY_NORMAL,
    };
    let result = if reportable {
        ctx.store.save_sample(&reading, now).map(|_| ())
    } else {
        // 回退样本保留为最新现场读数，但不进入可上报历史。
        ctx.store.save_reading(&reading, now)
    };
    if let Err(e) = result {
        tracing::warn!("读数落库失败: {e}");
    }
}

/// 周期性把所有尚未归批的中间样本装入一条 DATA_UPLOAD。SQLite 事务保证样本与 outbox 不会脱节。
async fn report_batch_task(ctx: TaskCtx, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(next_report_boundary(ctx.now_ms(), ctx.report_interval_s)) => flush_sample_batch(&ctx).await,
        }
    }
}

fn next_report_boundary(now_ms: u64, interval_s: u64) -> Duration {
    let interval_ms = interval_s.max(1).saturating_mul(1_000);
    let next_ms = (now_ms / interval_ms + 1).saturating_mul(interval_ms);
    Duration::from_millis(next_ms.saturating_sub(now_ms))
}

async fn flush_sample_batch(ctx: &TaskCtx) {
    let Ok(rows) = ctx.store.unbatched_samples(2_000) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    let mut samples = Vec::with_capacity(rows.len());
    let mut sample_ids = Vec::with_capacity(rows.len());
    for row in rows {
        let Ok(points) = serde_json::from_str::<Vec<(String, f64)>>(&row.points_json) else {
            ctx.emitter.emit(
                "ERROR",
                "batch",
                format!("历史样本 {} 无法解析，暂不归批", row.id),
            );
            continue;
        };
        samples.push(proto::MeterSample {
            device_sn: row.device_sn,
            modbus_addr: row.modbus_addr,
            channel_id: row.channel_id,
            profile_key: row.profile,
            model_version: row.model_version,
            config_revision: row.config_revision,
            collect_time: row.read_ms,
            sample_interval_seconds: row.sample_interval_s,
            quality: row.quality,
            points: points.into_iter().collect(),
        });
        sample_ids.push(row.id);
    }
    if samples.is_empty() {
        return;
    }
    let trusted = ctx.clock_trusted();
    let ts = ctx.state.next_message_ts(ctx.now_ms());
    let status = if trusted { "pending" } else { "deferred" };
    let gateway_id = ctx.state.gateway_id();
    let message_id = if trusted {
        proto::ids::data_message_id(&gateway_id, ts)
    } else {
        format!("DFR-BATCH-{}", ctx.state.next_defer_seq())
    };
    let payload = proto::GatewayUploadPayload::new_data(
        message_id.clone(),
        ctx.state.gateway_sn(),
        ts,
        ctx.report_interval_s,
        samples,
    );
    let Ok(bytes) = serde_json::to_vec(&payload) else {
        tracing::error!("报文序列化失败: {message_id}");
        return;
    };
    let item = gw_store::NewOutbox {
        message_id,
        topic: topics::data_upload_topic(&gateway_id),
        payload: bytes,
        created_ms: ctx.now_ms(),
    };
    match ctx.store.enqueue_sample_batch(&item, status, &sample_ids) {
        Ok(true) => {
            ctx.notify_publish.notify_one();
            ctx.emitter.emit(
                "INFO",
                "batch",
                format!("已归批 {} 个采样点", sample_ids.len()),
            );
        }
        Ok(false) => {}
        Err(e) => tracing::warn!("outbox 入队失败: {e}"),
    }
}

/// 采集失败：连续 3 次判离线。
fn handle_poll_failure(ctx: &TaskCtx, meter: &MeterRuntime, error: gw_collector::TransportError) {
    let mut went_offline = false;
    ctx.state.update_meter(meter.record.id, |m| {
        m.consecutive_failures += 1;
        if m.consecutive_failures == 3 {
            m.online = false;
            went_offline = true;
        }
    });
    if went_offline {
        raise_gateway_alarm(
            ctx,
            meter,
            &format!("DEVICE_OFFLINE:{}", meter.record.device_sn),
            "DEVICE_OFFLINE",
            "WARN",
            format!("{} 连续 3 次采集失败", meter.record.device_sn),
        );
        ctx.emitter.emit(
            "WARN",
            "网关告警",
            format!(
                "{} 连续 3 次采集失败，判定离线（缺采由云侧断采告警接管）",
                meter.record.device_sn
            ),
        );
    }
    tracing::debug!("采集失败 {}: {error}", meter.record.device_sn);
}

fn raise_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    alarm_type: &str,
    level: &str,
    message: String,
) {
    let now = ctx.now_ms();
    let row = gw_store::AlarmEventRecord {
        source_event_id: event_id.to_string(),
        source: "GATEWAY".into(),
        meter_id: Some(meter.record.id),
        alarm_type: alarm_type.into(),
        level: level.into(),
        point_code: None,
        message: message.clone(),
        status: "ACTIVE".into(),
        first_seen_ms: now,
        last_seen_ms: now,
        cloud_status: "LOCAL".into(),
        cloud_message_id: None,
        cloud_ack_ms: None,
    };
    if matches!(ctx.store.upsert_alarm(&row), Ok(true)) {
        enqueue_gateway_alarm(ctx, meter, &row, "RAISED", message);
    }
}

fn recover_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    event_id: &str,
    alarm_type: &str,
    message: String,
) {
    if matches!(
        ctx.store.resolve_alarm("GATEWAY", event_id, ctx.now_ms()),
        Ok(true)
    ) {
        let row = gw_store::AlarmEventRecord {
            source_event_id: event_id.to_string(),
            source: "GATEWAY".into(),
            meter_id: Some(meter.record.id),
            alarm_type: alarm_type.into(),
            level: "INFO".into(),
            point_code: None,
            message: message.clone(),
            status: "RECOVERED".into(),
            first_seen_ms: ctx.now_ms(),
            last_seen_ms: ctx.now_ms(),
            cloud_status: "LOCAL".into(),
            cloud_message_id: None,
            cloud_ack_ms: None,
        };
        enqueue_gateway_alarm(ctx, meter, &row, "RECOVERED", message);
    }
}

fn enqueue_gateway_alarm(
    ctx: &TaskCtx,
    meter: &MeterRuntime,
    alarm: &gw_store::AlarmEventRecord,
    action: &str,
    message: String,
) {
    let timestamp = ctx.state.next_message_ts(ctx.now_ms());
    let gateway_id = ctx.state.gateway_id();
    let message_id = proto::ids::alarm_message_id(&gateway_id, timestamp);
    let payload = proto::GatewayAlarmPayload {
        schema_version: proto::model::SCHEMA_VERSION.into(),
        message_id: message_id.clone(),
        gateway_sn: ctx.state.gateway_sn(),
        timestamp,
        kind: proto::model::TYPE_ALARM_UPLOAD.into(),
        event_id: alarm.source_event_id.clone(),
        action: action.into(),
        alarm_type: alarm.alarm_type.clone(),
        level: alarm.level.clone(),
        device_sn: Some(meter.record.device_sn.clone()),
        point_code: alarm.point_code.clone(),
        message,
    };
    let Ok(payload) = serde_json::to_vec(&payload) else {
        return;
    };
    let item = gw_store::NewOutbox {
        message_id: message_id.clone(),
        topic: topics::alarm_upload_topic(&gateway_id),
        payload,
        created_ms: timestamp,
    };
    if matches!(ctx.store.enqueue(&item), Ok(true)) {
        let _ = ctx
            .store
            .mark_alarm_cloud_pending(&alarm.source_event_id, &message_id);
        ctx.notify_publish.notify_one();
    }
}

/// 发布器：唯一出网出口，按 id 序发布；deferred 行在时钟可信后定稿。
async fn publisher_task(ctx: TaskCtx, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {}
        }
        if !ctx.clock_trusted() {
            continue; // 时钟不可信：不出网（deferred 行由采集侧持续暂存）
        }
        if !ctx.link.health().is_publishable() {
            continue;
        }
        // 1) 补时间戳：把 deferred 行定稿为 pending
        if let Ok(deferred) = ctx.store.deferred_outbox(64) {
            for row in deferred {
                let ts = ctx.state.next_message_ts(ctx.now_ms());
                match promote_deferred(&ctx, &row.payload, ts) {
                    Ok((message_id, payload)) => {
                        if let Err(e) = ctx.store.finalize_outbox(row.id, &message_id, &payload, ts)
                        {
                            tracing::warn!("deferred 定稿失败 id={}: {e}", row.id);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("deferred 报文解析失败 id={}: {e}", row.id);
                        let _ = ctx.store.mark_sent(row.id); // 坏行不阻塞队列
                    }
                }
            }
        }
        // 2) 按序发布
        let Ok(pending) = ctx.store.pending_outbox(16) else {
            continue;
        };
        for row in pending {
            match ctx.link.publish(&row.topic, &row.payload).await {
                Ok(()) => {
                    let _ = ctx.store.mark_sent(row.id);
                    if row.topic.ends_with("/alarm/up") {
                        if let Ok(alarm) =
                            serde_json::from_slice::<proto::GatewayAlarmPayload>(&row.payload)
                        {
                            let _ = ctx.store.mark_alarm_cloud_ack(
                                &alarm.event_id,
                                "MQTT_ACKED",
                                ctx.now_ms(),
                            );
                        }
                    }
                    ctx.state.mark_published(ctx.now_ms());
                    tracing::debug!("已发布 {} ({})", row.message_id, row.topic);
                }
                Err(e) => {
                    let _ = ctx.store.mark_attempt_failed(row.id, &e.to_string());
                    tracing::debug!("发布失败 {} : {e}", row.message_id);
                    break; // 保持顺序：失败即本轮停止，下轮重试
                }
            }
        }
    }
}

/// deferred 行定稿：时钟恢复后把整批样本按原相对间隔平移到可信时间，既避免旧时间被云侧拒绝，也不丢中间样本顺序。
fn promote_deferred(
    ctx: &TaskCtx,
    payload: &[u8],
    ts: u64,
) -> Result<(String, Vec<u8>), serde_json::Error> {
    let mut upload: proto::GatewayUploadPayload = serde_json::from_slice(payload)?;
    upload.timestamp = ts;
    let latest = upload
        .meters
        .iter()
        .map(|meter| meter.collect_time)
        .max()
        .unwrap_or(0);
    for meter in &mut upload.meters {
        let behind = latest.saturating_sub(meter.collect_time);
        meter.collect_time = ts.saturating_sub(behind);
    }
    let gateway_id = ctx.state.gateway_id();
    upload.message_id = proto::ids::data_message_id(&gateway_id, ts);
    let bytes = serde_json::to_vec(&upload)?;
    Ok((upload.message_id, bytes))
}

/// 心跳任务：仅在当时钟可信时发送。
async fn heartbeat_task(ctx: TaskCtx, shutdown: CancellationToken) {
    loop {
        let heartbeat_s = ctx.state.heartbeat_s().max(1);
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(heartbeat_s)) => {}
        }
        if !ctx.clock_trusted() {
            continue;
        }
        let now = ctx.state.next_message_ts(ctx.now_ms());
        ctx.state.mark_heartbeat(now);
        if !ctx.link.health().is_publishable() {
            continue;
        }
        let gateway_id = ctx.state.gateway_id();
        let devices = ctx
            .state
            .meters_snapshot()
            .into_iter()
            .map(|meter| proto::DeviceHeartbeat {
                device_sn: meter.record.device_sn,
                online: meter.online,
                last_read_time: meter.last_read_ms,
                quality: meter.last_quality,
            })
            .collect();
        let pending = ctx.store.outbox_pending_count().unwrap_or(0);
        let payload = proto::HeartbeatPayload::new_online(
            proto::ids::heartbeat_message_id(&gateway_id, now),
            ctx.state.gateway_sn(),
            now,
            devices,
            pending,
        );
        let Ok(bytes) = serde_json::to_vec(&payload) else {
            continue;
        };
        let topic = topics::heartbeat_topic(&gateway_id);
        if let Err(e) = ctx.link.publish(&topic, &bytes).await {
            tracing::debug!("心跳发布失败: {e}");
        }
    }
}

/// 指令执行任务：消费云链路下行。
async fn commander_task(
    ctx: TaskCtx,
    reboot: Arc<dyn RebootHook>,
    mut command_rx: mpsc::Receiver<RawCommand>,
    shutdown: CancellationToken,
) {
    loop {
        let raw = tokio::select! {
            _ = shutdown.cancelled() => return,
            received = command_rx.recv() => match received {
                Some(raw) => raw,
                None => return,
            },
        };
        handle_raw_command(&ctx, &reboot, raw).await;
    }
}

async fn handle_raw_command(ctx: &TaskCtx, reboot: &Arc<dyn RebootHook>, raw: RawCommand) {
    let now = ctx.now_ms();
    if raw.topic.ends_with("/alarm/ack") {
        match serde_json::from_slice::<proto::AlarmAckPayload>(&raw.payload) {
            Ok(ack) => {
                let _ = ctx
                    .store
                    .mark_alarm_cloud_ack(&ack.event_id, &ack.status, now);
                ctx.emitter.emit(
                    if matches!(
                        ack.status.as_str(),
                        "MQTT_ACKED" | "ACCESS_FORWARDED" | "DATA_CONSUMED"
                    ) {
                        "INFO"
                    } else {
                        "WARN"
                    },
                    "alarm-ack",
                    format!("告警 {} 平台回执：{}", ack.event_id, ack.status),
                );
            }
            Err(error) => {
                ctx.emitter
                    .emit("WARN", "alarm-ack", format!("告警回执解析失败: {error}"))
            }
        }
        return;
    }
    let parsed: Result<proto::CommandBody, _> = serde_json::from_slice(&raw.payload);
    let body = match parsed {
        Ok(body) => body,
        Err(e) => {
            ctx.emitter
                .emit("WARN", "command", format!("指令解析失败: {e}"));
            return;
        }
    };
    if body.command_id.is_empty() {
        return;
    }
    if let Ok(Some((status, message))) = ctx.store.command_result(&body.command_id) {
        let response = if status == "SUCCESS" {
            proto::CommandResponse::success(&body.command_id, message, now)
        } else {
            proto::CommandResponse::failed(&body.command_id, message, now)
        };
        let topic = topics::command_response_topic(&ctx.state.gateway_id());
        if let Ok(bytes) = serde_json::to_vec(&response) {
            let _ = ctx.link.publish(&topic, &bytes).await;
        }
        return;
    }
    let action = command::interpret(&body);
    let gateway_id = ctx.state.gateway_id();
    let response = match &action {
        GatewayAction::ReadNow { target_sn } => match ctx.state.meter_by_sn(target_sn) {
            Some(meter) if meter.record.enabled => {
                let rows = ctx
                    .store
                    .register_map(&meter.record.profile)
                    .unwrap_or_default();
                let specs = specs_of(&rows);
                match ctx.transports.get(&meter.record.channel_id) {
                    Some(transport) => match poll_once(&transport, &meter.record, &specs).await {
                        Ok(points) => {
                            handle_sample(ctx, &meter, points).await;
                            proto::CommandResponse::success(&body.command_id, "已读取并出网", now)
                        }
                        Err(e) => proto::CommandResponse::failed(
                            &body.command_id,
                            format!("读取失败: {e}"),
                            now,
                        ),
                    },
                    None => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("RS485 通道 {} 未建立传输", meter.record.channel_id),
                        now,
                    ),
                }
            }
            _ => proto::CommandResponse::failed(
                &body.command_id,
                format!("目标设备不存在或停用: {target_sn}"),
                now,
            ),
        },
        GatewayAction::SetInterval { seconds } => match policy::validate_collect_interval(*seconds)
        {
            Ok(validated) => {
                apply_interval(ctx, validated).await;
                proto::CommandResponse::success(
                    &body.command_id,
                    format!("采集周期已设为 {validated}s"),
                    now,
                )
            }
            Err(e) => proto::CommandResponse::failed(&body.command_id, e, now),
        },
        GatewayAction::Reboot => {
            let response = proto::CommandResponse::success(&body.command_id, "网关即将重启", now);
            let reboot = reboot.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                reboot.reboot();
            });
            response
        }
        GatewayAction::DeviceCommand { target_sn, command_code } => {
            if command_code.is_empty() {
                proto::CommandResponse::failed(&body.command_id, "commandCode 不能为空", now)
            } else if let Some(meter) = ctx.state.meter_by_sn(target_sn) {
                match ctx.store.protocol_command(&meter.record.profile, command_code) {
                    Ok(Some(command)) if command.function_code == 6
                        && command.encode_type.eq_ignore_ascii_case("FIXED")
                        && command.fixed_value.is_some() => {
                        match ctx.transports.get(&meter.record.channel_id) {
                            Some(transport) => {
                                let mut guard = transport.lock().await;
                                match guard.write_single_register(
                                    meter.record.modbus_addr,
                                    command.register_address,
                                    command.fixed_value.unwrap_or_default(),
                                ).await {
                                    Ok(()) => proto::CommandResponse::success(
                                        &body.command_id,
                                        format!("已执行 {}", command.command_name),
                                        now,
                                    ),
                                    Err(error) => proto::CommandResponse::failed(
                                        &body.command_id,
                                        format!("设备写入失败: {error}"),
                                        now,
                                    ),
                                }
                            }
                            None => proto::CommandResponse::failed(
                                &body.command_id,
                                format!("RS485 通道 {} 未建立传输", meter.record.channel_id),
                                now,
                            ),
                        }
                    }
                    Ok(Some(_)) => proto::CommandResponse::failed(
                        &body.command_id,
                        "该白名单命令的写入类型暂不受支持",
                        now,
                    ),
                    Ok(None) => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("命令未在设备协议白名单中: {command_code}"),
                        now,
                    ),
                    Err(error) => proto::CommandResponse::failed(
                        &body.command_id,
                        format!("命令白名单读取失败: {error}"),
                        now,
                    ),
                }
            } else {
                proto::CommandResponse::failed(
                    &body.command_id,
                    format!("目标设备不存在或停用: {target_sn}"),
                    now,
                )
            }
        }
        GatewayAction::Unknown { kind } => proto::CommandResponse::failed(
            &body.command_id,
            format!("不支持的指令类型: {kind}"),
            now,
        ),
    };

    let _ = ctx.store.record_command(&gw_store::CommandLogRow {
        command_id: body.command_id.clone(),
        command_type: action.name().to_string(),
        target_sn: body.target_sn.clone().unwrap_or_default(),
        payload_json: body.payload.to_string(),
        received_ms: now,
        result_status: response.status.clone(),
        responded_ms: now,
        message: response.message.clone(),
    });
    ctx.emitter.emit(
        "INFO",
        "command",
        format!(
            "指令 {} {} → {}",
            body.command_id,
            action.name(),
            response.status
        ),
    );
    let topic = topics::command_response_topic(&gateway_id);
    if let Ok(bytes) = serde_json::to_vec(&response) {
        if let Err(e) = ctx.link.publish(&topic, &bytes).await {
            tracing::warn!("指令回执发布失败 {}: {e}", body.command_id);
        }
    }
}

/// SET_INTERVAL：全局采集周期（所有表 + 默认值）。
async fn apply_interval(ctx: &TaskCtx, seconds: u64) {
    let _ = ctx
        .store
        .set_config("data_interval_s", &seconds.to_string());
    for meter in ctx.state.meters_snapshot() {
        let mut input = gw_store::MeterInput {
            device_sn: meter.record.device_sn.clone(),
            device_name: meter.record.device_name.clone(),
            modbus_addr: meter.record.modbus_addr,
            profile: meter.record.profile.clone(),
            channel_id: meter.record.channel_id.clone(),
            upload_enabled: meter.record.upload_enabled,
            collect_interval_s: seconds,
            enabled: meter.record.enabled,
        };
        input.collect_interval_s = seconds;
        let _ = ctx.store.update_meter(meter.record.id, &input);
        ctx.state.update_meter(meter.record.id, |m| {
            m.record.collect_interval_s = seconds;
        });
    }
    // 周期变化无需重启任务：poller 每轮从状态重读周期
    ctx.emitter.emit(
        "INFO",
        "command",
        format!("全部电表采集周期已调整为 {seconds}s"),
    );
}

/// UI 命令处理任务。
async fn user_command_task(
    ctx: TaskCtx,
    mut cmd_rx: mpsc::Receiver<crate::ui_command::UserCommand>,
    shutdown: CancellationToken,
) {
    loop {
        let command = tokio::select! {
            _ = shutdown.cancelled() => return,
            received = cmd_rx.recv() => match received {
                Some(command) => command,
                None => return,
            },
        };
        use crate::ui_command::UserCommand;
        match command {
            UserCommand::SaveLink {
                gateway_id,
                gateway_sn,
                mqtt_host,
                mqtt_port,
                mqtt_username,
                mqtt_password,
            } => {
                let _ = ctx.store.set_config("mqtt_host", &mqtt_host);
                let _ = ctx.store.set_config("mqtt_port", &mqtt_port.to_string());
                let _ = ctx.store.set_config("mqtt_username", &mqtt_username);
                let _ = ctx.store.set_config("mqtt_password", &mqtt_password);
                let _ = ctx.store.set_config("gateway_id", &gateway_id);
                let _ = ctx.store.set_config("gateway_sn", &gateway_sn);
                ctx.state.set_identity(gateway_id, gateway_sn);
                ctx.emitter.emit(
                    "INFO",
                    "config",
                    "云端参数已保存；broker 连接参数重启服务后生效",
                );
            }
            UserCommand::SetHeartbeat { seconds } => {
                match policy::validate_heartbeat_interval(seconds) {
                    Ok(validated) => {
                        let _ = ctx.store.set_config("heartbeat_s", &validated.to_string());
                        ctx.state.set_heartbeat_s(validated);
                        ctx.emitter
                            .emit("INFO", "config", format!("心跳周期已设为 {validated}s"));
                    }
                    Err(e) => ctx.emitter.emit("WARN", "config", e),
                }
            }
            UserCommand::SaveMeter { id, input } => {
                if let Err(e) = policy::validate_device_sn(&input.device_sn).and_then(|_| {
                    policy::validate_collect_interval(input.collect_interval_s).map(|_| ())
                }) {
                    ctx.emitter
                        .emit("WARN", "config", format!("电表参数非法: {e}"));
                    continue;
                }
                let result = match id {
                    Some(id) => ctx.store.update_meter(id, &input).map(|_| id),
                    None => ctx.store.insert_meter(&input, ctx.now_ms()),
                };
                match result {
                    Ok(saved_id) => {
                        reload_meter_runtime(&ctx, saved_id);
                        ctx.sync_pollers(&shutdown);
                        ctx.emitter.emit(
                            "INFO",
                            "config",
                            format!("电表 {} 已保存", input.device_sn),
                        );
                    }
                    Err(e) => ctx
                        .emitter
                        .emit("WARN", "config", format!("电表保存失败: {e}")),
                }
            }
            UserCommand::DeleteMeter { id } => match ctx.store.delete_meter(id) {
                Ok(()) => {
                    let meters: Vec<MeterRuntime> = ctx
                        .store
                        .meters()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|record| {
                            let reading = ctx.store.reading(record.id).unwrap_or(None);
                            MeterRuntime::from_record(record, reading)
                        })
                        .collect();
                    ctx.state.replace_meters(meters);
                    ctx.sync_pollers(&shutdown);
                    ctx.emitter.emit("INFO", "config", "电表已删除");
                }
                Err(e) => ctx.emitter.emit("WARN", "config", format!("删除失败: {e}")),
            },
            UserCommand::ReloadConfiguration => {
                let meters: Vec<MeterRuntime> = ctx
                    .store
                    .meters()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|record| {
                        let reading = ctx.store.reading(record.id).unwrap_or(None);
                        MeterRuntime::from_record(record, reading)
                    })
                    .collect();
                ctx.state.replace_meters(meters);
                ctx.sync_pollers(&shutdown);
                ctx.emitter
                    .emit("INFO", "sync", "平台物模型与设备配置已应用");
            }
            UserCommand::ReadNow { meter_id } => match ctx.state.meter_by_id(meter_id) {
                Some(meter) if meter.record.enabled => {
                    let rows = ctx
                        .store
                        .register_map(&meter.record.profile)
                        .unwrap_or_default();
                    let specs = specs_of(&rows);
                    let Some(transport) = ctx.transports.get(&meter.record.channel_id) else {
                        handle_channel_unavailable(&ctx, &meter);
                        continue;
                    };
                    match poll_once(&transport, &meter.record, &specs).await {
                        Ok(points) => {
                            handle_sample(&ctx, &meter, points).await;
                            ctx.emitter.emit(
                                "INFO",
                                "command",
                                format!("{} 手动读取完成", meter.record.device_sn),
                            );
                        }
                        Err(e) => {
                            let detail = e.to_string();
                            handle_poll_failure(&ctx, &meter, e);
                            ctx.emitter.emit(
                                "WARN",
                                "command",
                                format!("{} 手动读取失败: {detail}", meter.record.device_sn),
                            );
                        }
                    }
                }
                _ => ctx.emitter.emit("WARN", "command", "目标电表不存在或停用"),
            },
            UserCommand::SetContinuousPull { meter_id, enabled } => {
                let Some(meter) = ctx.state.meter_by_id(meter_id) else {
                    ctx.emitter.emit("WARN", "command", "目标电表不存在");
                    continue;
                };
                if !meter.record.enabled {
                    ctx.emitter.emit("WARN", "command", "目标电表已停用");
                    continue;
                }
                ctx.state.set_continuous_pull(meter_id, enabled);
                ctx.registry.cancel(meter_id);
                ctx.sync_pollers(&shutdown);
                ctx.emitter.emit(
                    "INFO",
                    "command",
                    format!(
                        "{}{}持续拉取",
                        meter.record.device_sn,
                        if enabled {
                            " 已开始每 5 秒"
                        } else {
                            " 已停止"
                        }
                    ),
                );
            }
            UserCommand::DiagnoseCloud => {
                let health = ctx.link.health();
                let clock_ok = ctx.clock_trusted();
                ctx.emitter.emit(
                    "INFO",
                    "link",
                    format!(
                        "云链路诊断：{}，时钟{}",
                        health.label(),
                        if clock_ok {
                            "可信"
                        } else {
                            "未同步（数据暂存不出网）"
                        }
                    ),
                );
            }
        }
    }
}

/// 档案变更后从库中重载单表运行时（保留在线状态位）。
fn reload_meter_runtime(ctx: &TaskCtx, meter_id: i64) {
    let Some(record) = ctx
        .store
        .meters()
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == meter_id)
    else {
        return;
    };
    let reading = ctx.store.reading(meter_id).unwrap_or(None);
    let mut reloaded = MeterRuntime::from_record(record, reading);
    if let Some(existing) = ctx.state.meter_by_id(meter_id) {
        reloaded.online = existing.online;
        reloaded.last_total_kwh = existing.last_total_kwh.or(reloaded.last_total_kwh);
        reloaded.last_points = existing.last_points;
        reloaded.last_read_ms = existing.last_read_ms;
    }
    let mut meters = ctx.state.meters_snapshot();
    if let Some(slot) = meters.iter_mut().find(|m| m.record.id == meter_id) {
        *slot = reloaded;
    } else {
        meters.push(reloaded);
    }
    ctx.state.replace_meters(meters);
}

/// 快照任务：1Hz 聚合状态给观察者。
async fn snapshotter_task(
    ctx: TaskCtx,
    snapshot_tx: watch::Sender<Snapshot>,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {}
        }
        let now = ctx.now_ms();
        let day = energy::day_key(now, ctx.tz_offset_s);
        let (online, total) = ctx.state.online_counts();
        let meters: Vec<MeterSnapshot> = ctx
            .state
            .meters_snapshot()
            .into_iter()
            .map(|m| MeterSnapshot {
                id: m.record.id,
                device_sn: m.record.device_sn.clone(),
                online: m.online,
                addr: m.record.modbus_addr,
                interval_s: m.record.collect_interval_s,
                continuous_pull: ctx.state.continuous_pull_enabled(m.record.id),
                enabled: m.record.enabled,
                profile: m.record.profile.clone(),
                last_read_ms: m.last_read_ms,
                quality: m.last_quality,
                points: m.last_points.clone(),
                today_kwh: ctx.store.energy_of(m.record.id, &day).unwrap_or(0.0),
            })
            .collect();
        let snapshot = Snapshot {
            cloud: ctx.link.health(),
            clock_trusted: ctx.clock_trusted(),
            outbox_pending: ctx.store.outbox_pending_count().unwrap_or(0),
            outbox_sent_total: ctx.store.outbox_sent_count().unwrap_or(0),
            meters_online: online,
            meters_total: total,
            today_kwh_total: ctx.store.energy_sum(&day).unwrap_or(0.0),
            last_publish_ms: ctx.state.last_publish_ms(),
            last_heartbeat_ms: ctx.state.last_heartbeat_ms(),
            started_ms: ctx.state.started_ms,
            version: env!("CARGO_PKG_VERSION").to_string(),
            meters,
        };
        let _ = snapshot_tx.send(snapshot);
    }
}

/// 维护任务：sent 行 7 天滚动清理 + 事件裁剪。
async fn maintainer_task(
    store: Arc<Store>,
    clock: Arc<dyn ClockSource>,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(3600));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {}
        }
        let cutoff = clock.now_ms().saturating_sub(7 * 24 * 3600 * 1000);
        if let Err(e) = store.cleanup_samples_before(cutoff) {
            tracing::warn!("历史样本清理失败: {e}");
        }
        if let Err(e) = store.cleanup_local_samples_before(cutoff) {
            tracing::warn!("本地设备历史样本清理失败: {e}");
        }
        match store.cleanup_sent_before(cutoff) {
            Ok(n) if n > 0 => tracing::info!("清理 {n} 条已发布 outbox 行"),
            Ok(_) => {}
            Err(e) => tracing::warn!("outbox 清理失败: {e}"),
        }
        if let Err(e) = store.trim_events(5000) {
            tracing::warn!("事件裁剪失败: {e}");
        }
    }
}

/// re-export 供 bins 组装。
pub use gw_core::snapshot::LinkHealth as Health;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct BlockTransport(Arc<AtomicUsize>);

    #[async_trait]
    impl MeterTransport for BlockTransport {
        async fn read_registers(&mut self, _: u8, _: u8, _: u16, _: u16) -> Result<Vec<u16>, gw_collector::TransportError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![0x8001])
        }
    }

    fn bit_row(code: &str, bit: u8) -> gw_store::RegisterMapRow {
        gw_store::RegisterMapRow {
            profile: "LIGHTING".into(), point_code: code.into(), point_name: code.into(), unit: String::new(),
            func: 3, address: 0x007A, quantity: 1, field_offset: 0, field_quantity: 1,
            bit_offset: Some(bit), bit_length: Some(1), data_type: "boolean".into(), byte_order: "AB".into(),
            scale: 1.0, offset: 0.0,
        }
    }

    #[tokio::test]
    async fn shared_status_block_is_read_once_and_split_into_bits() {
        let calls = Arc::new(AtomicUsize::new(0));
        let transport = Mutex::new(BlockTransport(calls.clone()));
        let meter = MeterRecord {
            id: 1, device_sn: "LIGHT-001".into(), device_name: "light".into(), modbus_addr: 7,
            profile: "LIGHTING".into(), channel_id: "rs485-1".into(), config_source: "PLATFORM".into(),
            platform_device_id: Some(1), model_version: "V1".into(), upload_enabled: true,
            collect_interval_s: 60, enabled: true, created_ms: 0,
        };
        let points = poll_once(&transport, &meter, &[bit_row("LOOP_1_STATUS", 15), bit_row("LOOP_16_STATUS", 0)])
            .await.expect("block should decode");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(points, vec![("LOOP_1_STATUS".into(), 1.0), ("LOOP_16_STATUS".into(), 1.0)]);
    }
}
