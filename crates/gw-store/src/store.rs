mod weekly;
mod validation;
use validation::*;
#[cfg(test)]
mod tests;

mod schema;
use schema::*;
mod samples;

mod platform_sync;

mod outbox;

mod models;

mod meters;

mod journal;

mod energy;

mod configuration;

mod channels;

mod alarm_rules;

mod alarm_protocols;

mod alarm_events;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use crate::records::*;
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration validation failed: {0}")]
    Validation(String),
}
pub type StoreResult<T> = Result<T, StoreError>;
const STAGING_CHANNEL_ID: &str = "__staging__";
/// SQLite 存储门面。`Send` 但内部以 Mutex 串行化（短临界区）。
pub struct Store {
    conn: Mutex<Connection>,
}
impl Store {
    /// 打开（或创建）文件库，启用 WAL 与外键约束。
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }
    fn init(conn: Connection) -> StoreResult<Self> {
        conn.busy_timeout(std::time::Duration::from_millis(3_000))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn: Mutex::new(conn) };
        store.migrate()?;
        Ok(store)
    }
    fn migrate(&self) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, script) in MIGRATIONS.iter().enumerate() {
            let target = (i + 1) as i64;
            if version < target {
                conn.execute_batch(script)?;
                conn.pragma_update(None, "user_version", target)?;
            }
        }
        Ok(())
    }
}
