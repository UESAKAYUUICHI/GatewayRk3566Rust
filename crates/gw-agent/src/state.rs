//! 运行时状态：agent 各任务共享的可变视图（RwLock 保护，短临界区）。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use gw_proto::points::FORWARD_ACTIVE_ENERGY;
use gw_store::MeterRecord;

/// 单表运行时视图。
#[derive(Debug, Clone)]
pub struct MeterRuntime {
    pub record: MeterRecord,
    pub online: bool,
    pub consecutive_failures: u32,
    pub last_read_ms: u64,
    pub last_quality: u32,
    pub last_points: Vec<(String, f64)>,
    /// 上一次正向有功总电能（kWh），能量单调基线
    pub last_total_kwh: Option<f64>,
}

impl MeterRuntime {
    /// 由档案与库中最新读数快照构造（冷启动恢复基线）。
    pub fn from_record(record: MeterRecord, last_reading: Option<gw_store::ReadingRecord>) -> Self {
        let (last_read_ms, last_quality, last_points) = match last_reading {
            Some(r) => {
                let points: Vec<(String, f64)> =
                    serde_json::from_str::<serde_json::Value>(&r.snapshot_json)
                        .map(|v| {
                            v.as_object()
                                .map(|obj| {
                                    obj.iter()
                                        .filter_map(|(k, val)| val.as_f64().map(|f| (k.clone(), f)))
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                (r.read_ms, r.quality, points)
            }
            None => (0, 0, Vec::new()),
        };
        let last_total_kwh = last_points
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(FORWARD_ACTIVE_ENERGY))
            .map(|(_, v)| *v);
        Self {
            record,
            online: false,
            consecutive_failures: 0,
            last_read_ms,
            last_quality,
            last_points,
            last_total_kwh,
        }
    }
}

struct Inner {
    gateway_id: String,
    gateway_sn: String,
    heartbeat_s: u64,
    meters: Vec<MeterRuntime>,
    last_publish_ms: u64,
    last_heartbeat_ms: u64,
    continuous_pull_ids: HashSet<i64>,
}

/// 共享状态。
pub struct AppState {
    inner: RwLock<Inner>,
    pub started_ms: u64,
    /// 单调报文时间戳分配器：保证 messageId 全局唯一（同毫秒多报文时 +1 递增）
    message_ts: AtomicU64,
    /// 进程内延迟行序号
    defer_seq: AtomicU64,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(
        gateway_id: String,
        gateway_sn: String,
        heartbeat_s: u64,
        meters: Vec<MeterRuntime>,
        started_ms: u64,
    ) -> Self {
        Self {
            inner: RwLock::new(Inner {
                gateway_id,
                gateway_sn,
                heartbeat_s,
                meters,
                last_publish_ms: 0,
                last_heartbeat_ms: 0,
                continuous_pull_ids: HashSet::new(),
            }),
            started_ms,
            message_ts: AtomicU64::new(started_ms),
            defer_seq: AtomicU64::new(0),
        }
    }

    /// 分配一个严格递增的报文时间戳（不低于 now）。
    pub fn next_message_ts(&self, now_ms: u64) -> u64 {
        loop {
            let current = self.message_ts.load(Ordering::Relaxed);
            let next = now_ms.max(current + 1);
            if self
                .message_ts
                .compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return next;
            }
        }
    }

    pub fn next_defer_seq(&self) -> u64 {
        self.defer_seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn gateway_id(&self) -> String {
        self.inner
            .read()
            .expect("state poisoned")
            .gateway_id
            .clone()
    }

    pub fn gateway_sn(&self) -> String {
        self.inner
            .read()
            .expect("state poisoned")
            .gateway_sn
            .clone()
    }

    pub fn heartbeat_s(&self) -> u64 {
        self.inner.read().expect("state poisoned").heartbeat_s
    }

    pub fn set_heartbeat_s(&self, seconds: u64) {
        self.inner.write().expect("state poisoned").heartbeat_s = seconds;
    }

    pub fn set_identity(&self, gateway_id: String, gateway_sn: String) {
        let mut inner = self.inner.write().expect("state poisoned");
        inner.gateway_id = gateway_id;
        inner.gateway_sn = gateway_sn;
    }

    pub fn mark_published(&self, ts_ms: u64) {
        self.inner.write().expect("state poisoned").last_publish_ms = ts_ms;
    }

    pub fn mark_heartbeat(&self, ts_ms: u64) {
        self.inner
            .write()
            .expect("state poisoned")
            .last_heartbeat_ms = ts_ms;
    }

    pub fn last_publish_ms(&self) -> u64 {
        self.inner.read().expect("state poisoned").last_publish_ms
    }

    pub fn last_heartbeat_ms(&self) -> u64 {
        self.inner.read().expect("state poisoned").last_heartbeat_ms
    }

    pub fn meters_snapshot(&self) -> Vec<MeterRuntime> {
        self.inner.read().expect("state poisoned").meters.clone()
    }

    pub fn enabled_meters(&self) -> Vec<MeterRuntime> {
        let meters = self.meters_snapshot();
        meters.into_iter().filter(|m| m.record.enabled).collect()
    }

    pub fn meter_by_id(&self, id: i64) -> Option<MeterRuntime> {
        self.inner
            .read()
            .expect("state poisoned")
            .meters
            .iter()
            .find(|m| m.record.id == id)
            .cloned()
    }

    pub fn meter_by_sn(&self, sn: &str) -> Option<MeterRuntime> {
        self.inner
            .read()
            .expect("state poisoned")
            .meters
            .iter()
            .find(|m| m.record.device_sn == sn)
            .cloned()
    }

    /// 以函数更新单表运行时（采样成功/失败路径共用）。
    pub fn update_meter(&self, id: i64, update: impl FnOnce(&mut MeterRuntime)) {
        let mut inner = self.inner.write().expect("state poisoned");
        if let Some(meter) = inner.meters.iter_mut().find(|m| m.record.id == id) {
            update(meter);
        }
    }

    /// 用最新档案整体替换 meters（档案变更后调用，随后 sync_pollers 重启采集任务）。
    pub fn replace_meters(&self, meters: Vec<MeterRuntime>) {
        let mut inner = self.inner.write().expect("state poisoned");
        let ids: HashSet<i64> = meters.iter().map(|meter| meter.record.id).collect();
        inner.continuous_pull_ids.retain(|id| ids.contains(id));
        inner.meters = meters;
    }

    pub fn set_continuous_pull(&self, meter_id: i64, enabled: bool) {
        let mut inner = self.inner.write().expect("state poisoned");
        if enabled {
            inner.continuous_pull_ids.insert(meter_id);
        } else {
            inner.continuous_pull_ids.remove(&meter_id);
        }
    }

    pub fn continuous_pull_enabled(&self, meter_id: i64) -> bool {
        self.inner
            .read()
            .expect("state poisoned")
            .continuous_pull_ids
            .contains(&meter_id)
    }

    pub fn online_counts(&self) -> (u32, u32) {
        let inner = self.inner.read().expect("state poisoned");
        let total = inner.meters.len() as u32;
        let online = inner.meters.iter().filter(|m| m.online).count() as u32;
        (online, total)
    }
}

/// 采集任务注册表：meter_id → 取消令牌（档案变更时按差异重启）。
#[derive(Default)]
pub struct PollerRegistry {
    tokens: RwLock<HashMap<i64, Arc<tokio_util::sync::CancellationToken>>>,
}

impl PollerRegistry {
    pub fn current_ids(&self) -> Vec<i64> {
        self.tokens
            .read()
            .expect("registry poisoned")
            .keys()
            .copied()
            .collect()
    }

    /// 为某表登记（或复用）取消令牌。
    pub fn token_for(&self, meter_id: i64) -> Arc<tokio_util::sync::CancellationToken> {
        let mut tokens = self.tokens.write().expect("registry poisoned");
        tokens
            .entry(meter_id)
            .or_insert_with(|| Arc::new(tokio_util::sync::CancellationToken::new()))
            .clone()
    }

    /// 取消并移除某表的登记（若存在）。
    pub fn cancel(&self, meter_id: i64) {
        if let Some(token) = self
            .tokens
            .write()
            .expect("registry poisoned")
            .remove(&meter_id)
        {
            token.cancel();
        }
    }
}
