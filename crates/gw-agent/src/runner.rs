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
#[cfg(test)]
mod tests;

mod sample_ingestion;
use sample_ingestion::*;
mod protocol_alarms;
use protocol_alarms::*;
mod maintenance;
use maintenance::*;
mod local_commands;
use local_commands::*;
mod collection_tasks;
use collection_tasks::*;
mod collection_bus;
use collection_bus::*;
mod cloud_delivery;
use cloud_delivery::*;
mod cloud_commands;
use cloud_commands::*;
mod bootstrap;
use bootstrap::*;
mod batch_reporting;
use batch_reporting::*;
mod alarm_delivery;
use alarm_delivery::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use gw_collector::profile::{DataType, MeterProfile, RegisterSpec};
use gw_collector::{MeterTransport, ModbusRtuTransport, SerialConfig};
use gw_core::clock::{ClockAssessor, ClockTrust};
use gw_core::command::{self, GatewayAction};
use gw_core::energy::{self, EnergyVerdict};
use gw_core::policy;
use gw_core::snapshot::{EventLine, MeterSnapshot, Snapshot};
use gw_link::port::{CloudLink, RawCommand};
use gw_proto::points::FORWARD_ACTIVE_ENERGY;
use gw_proto::{self as proto, topics};
use gw_store::{AlarmProtocolPointRecord, MeterInput, MeterRecord, Store};
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
        let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
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
        if let Err(e) = self
            .store
            .log_event(
                &gw_store::EventRow {
                    ts_ms: line.ts_ms,
                    level: line.level,
                    source: line.source,
                    message: line.message,
                },
            )
        {
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
    let AgentDeps { store, link, transports, clock, reboot, bootstrap } = deps;
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
    let state: SharedState = Arc::new(
        AppState::new(
            bootstrap.cloud.gateway_id.clone(),
            bootstrap.cloud.gateway_sn.clone(),
            heartbeat_s,
            meters,
            now,
        ),
    );
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
    emitter
        .emit("INFO", "agent", format!("网关启动 v{}", env!("CARGO_PKG_VERSION")));
    for meter in state.enabled_meters() {
        tokio::spawn(poller_task(ctx.clone(), meter.record.id, ctx.registry.token_for(meter.record.id), shutdown.clone()));
    }
    tokio::spawn(publisher_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(report_batch_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(heartbeat_task(ctx.clone(), shutdown.clone()));
    tokio::spawn(commander_task(ctx.clone(), reboot, command_rx, shutdown.clone()));
    tokio::spawn(user_command_task(ctx.clone(), cmd_rx, shutdown.clone()));
    tokio::spawn(snapshotter_task(ctx, snapshot_tx, shutdown.clone()));
    tokio::spawn(
        maintainer_task(
            store.clone(),
            clock.clone(),
            bootstrap.tz_offset_s,
            shutdown.clone(),
        ),
    );
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
                let token = self.registry.token_for(id);
                tokio::spawn(poller_task(self.clone(), id, token, shutdown.clone()));
            }
        }
    }
}
/// re-export 供 bins 组装。
pub use gw_core::snapshot::LinkHealth as Health;
