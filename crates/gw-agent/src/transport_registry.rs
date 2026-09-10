use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gw_collector::MeterTransport;
use tokio::sync::Mutex;

pub type TransportHandle = Arc<Mutex<dyn MeterTransport>>;

/// 每个 RS485 通道拥有独立的串口/锁，不同通道可并行采集，同一通道严格串行。
#[derive(Default)]
pub struct TransportRegistry {
    transports: RwLock<HashMap<String, TransportHandle>>,
}

impl TransportRegistry {
    pub fn insert(&self, channel_id: impl Into<String>, transport: TransportHandle) {
        self.transports
            .write()
            .expect("transport registry poisoned")
            .insert(channel_id.into(), transport);
    }

    pub fn remove(&self, channel_id: &str) {
        self.transports
            .write()
            .expect("transport registry poisoned")
            .remove(channel_id);
    }

    pub fn get(&self, channel_id: &str) -> Option<TransportHandle> {
        self.transports
            .read()
            .expect("transport registry poisoned")
            .get(channel_id)
            .cloned()
    }

    pub fn channel_ids(&self) -> Vec<String> {
        self.transports
            .read()
            .expect("transport registry poisoned")
            .keys()
            .cloned()
            .collect()
    }
}
