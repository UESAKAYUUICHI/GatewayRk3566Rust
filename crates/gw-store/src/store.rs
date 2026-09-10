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

const MIGRATIONS: &[&str] = &[
    // v1：初始结构
    "
    CREATE TABLE IF NOT EXISTS gateway_config (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS meter (
        id                 INTEGER PRIMARY KEY AUTOINCREMENT,
        device_sn          TEXT NOT NULL UNIQUE,
        modbus_addr        INTEGER NOT NULL,
        profile            TEXT NOT NULL DEFAULT 'PD666-3S3',
        collect_interval_s INTEGER NOT NULL DEFAULT 300,
        enabled            INTEGER NOT NULL DEFAULT 1,
        created_ms         INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS meter_register_map (
        profile    TEXT NOT NULL,
        point_code TEXT NOT NULL,
        func       INTEGER NOT NULL DEFAULT 3,
        address    INTEGER NOT NULL,
        quantity   INTEGER NOT NULL DEFAULT 1,
        data_type  TEXT NOT NULL DEFAULT 'u16',
        scale      REAL NOT NULL DEFAULT 1.0,
        offset     REAL NOT NULL DEFAULT 0.0,
        PRIMARY KEY (profile, point_code)
    );
    CREATE TABLE IF NOT EXISTS outbox_message (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id  TEXT NOT NULL UNIQUE,
        topic       TEXT NOT NULL,
        payload     BLOB NOT NULL,
        created_ms  INTEGER NOT NULL,
        status      TEXT NOT NULL DEFAULT 'pending',
        attempts    INTEGER NOT NULL DEFAULT 0,
        last_error  TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_outbox_status ON outbox_message(status, id);
    CREATE TABLE IF NOT EXISTS meter_reading (
        meter_id   INTEGER PRIMARY KEY REFERENCES meter(id) ON DELETE CASCADE,
        snapshot   TEXT NOT NULL,
        read_ms    INTEGER NOT NULL,
        quality    INTEGER NOT NULL DEFAULT 0,
        updated_ms INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS energy_daily (
        meter_id  INTEGER NOT NULL REFERENCES meter(id) ON DELETE CASCADE,
        day       TEXT NOT NULL,
        delta_kwh REAL NOT NULL DEFAULT 0,
        PRIMARY KEY (meter_id, day)
    );
    CREATE TABLE IF NOT EXISTS command_log (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        command_id     TEXT NOT NULL UNIQUE,
        command_type   TEXT,
        target_sn      TEXT,
        payload        TEXT,
        received_ms    INTEGER NOT NULL,
        result_status  TEXT,
        responded_ms   INTEGER,
        message        TEXT
    );
    CREATE TABLE IF NOT EXISTS event_log (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        ts_ms   INTEGER NOT NULL,
        level   TEXT NOT NULL,
        source  TEXT NOT NULL,
        message TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_event_ts ON event_log(ts_ms);
    ",
    // v2：物模型/通道绑定、完整采样历史、可靠归批与带来源的本地告警。
    "
    CREATE TABLE IF NOT EXISTS rs485_channel (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        port       TEXT NOT NULL,
        baud       INTEGER NOT NULL DEFAULT 9600,
        data_bits  INTEGER NOT NULL DEFAULT 8,
        stop_bits  INTEGER NOT NULL DEFAULT 1,
        parity     TEXT NOT NULL DEFAULT 'NONE',
        enabled    INTEGER NOT NULL DEFAULT 1,
        updated_ms INTEGER NOT NULL DEFAULT 0
    );
    INSERT OR IGNORE INTO rs485_channel(id,name,port,baud,data_bits,stop_bits,parity,enabled,updated_ms)
    VALUES('rs485-1','RS485-1','/dev/ttyS3',9600,8,1,'NONE',1,0);

    ALTER TABLE meter ADD COLUMN device_name TEXT NOT NULL DEFAULT '';
    ALTER TABLE meter ADD COLUMN channel_id TEXT NOT NULL DEFAULT 'rs485-1';
    ALTER TABLE meter ADD COLUMN config_source TEXT NOT NULL DEFAULT 'LOCAL';
    ALTER TABLE meter ADD COLUMN platform_device_id INTEGER;
    ALTER TABLE meter ADD COLUMN model_version TEXT NOT NULL DEFAULT 'local-v1';
    ALTER TABLE meter ADD COLUMN upload_enabled INTEGER NOT NULL DEFAULT 0;
    CREATE UNIQUE INDEX IF NOT EXISTS uk_meter_channel_addr
      ON meter(channel_id,modbus_addr)
      WHERE channel_id <> '__staging__';

    ALTER TABLE meter_register_map ADD COLUMN point_name TEXT NOT NULL DEFAULT '';
    ALTER TABLE meter_register_map ADD COLUMN unit TEXT NOT NULL DEFAULT '';

    CREATE TABLE IF NOT EXISTS thing_model (
        profile          TEXT PRIMARY KEY,
        name             TEXT NOT NULL,
        version          TEXT NOT NULL DEFAULT 'local-v1',
        source           TEXT NOT NULL DEFAULT 'LOCAL',
        platform_model_id INTEGER,
        content_hash     TEXT,
        enabled          INTEGER NOT NULL DEFAULT 1,
        updated_ms       INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS meter_sample (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        meter_id    INTEGER NOT NULL REFERENCES meter(id) ON DELETE CASCADE,
        read_ms     INTEGER NOT NULL,
        quality     INTEGER NOT NULL DEFAULT 0,
        points_json TEXT NOT NULL,
        outbox_id   INTEGER REFERENCES outbox_message(id),
        created_ms  INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_sample_meter_time ON meter_sample(meter_id,read_ms);
    CREATE INDEX IF NOT EXISTS idx_sample_unbatched ON meter_sample(outbox_id,id);

    CREATE TABLE IF NOT EXISTS alarm_event_local (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        source_event_id TEXT NOT NULL,
        source          TEXT NOT NULL CHECK(source IN ('GATEWAY','PLATFORM')),
        meter_id        INTEGER REFERENCES meter(id) ON DELETE SET NULL,
        alarm_type      TEXT NOT NULL,
        level           TEXT NOT NULL,
        point_code      TEXT,
        message         TEXT NOT NULL,
        status          TEXT NOT NULL DEFAULT 'ACTIVE',
        first_seen_ms   INTEGER NOT NULL,
        last_seen_ms    INTEGER NOT NULL,
        occurrence_count INTEGER NOT NULL DEFAULT 1,
        UNIQUE(source,source_event_id)
    );
    CREATE INDEX IF NOT EXISTS idx_alarm_local_status_time ON alarm_event_local(status,last_seen_ms);

    CREATE TABLE IF NOT EXISTS config_sync_state (
        id               INTEGER PRIMARY KEY CHECK(id=1),
        desired_revision TEXT,
        applied_revision TEXT,
        last_sync_ms     INTEGER,
        last_status      TEXT,
        last_error       TEXT
    );
    INSERT OR IGNORE INTO config_sync_state(id,last_status) VALUES(1,'NEVER');
    ",
    // v3：网关告警北向业务回执。
    "
    ALTER TABLE alarm_event_local ADD COLUMN cloud_message_id TEXT;
    ALTER TABLE alarm_event_local ADD COLUMN cloud_status TEXT NOT NULL DEFAULT 'LOCAL';
    ALTER TABLE alarm_event_local ADD COLUMN cloud_ack_ms INTEGER;
    ",
    // v4：移除早期演示用 SIM 设备，现场设备只保留平台同步或手工配置。
    "
    DELETE FROM meter WHERE device_sn LIKE 'SIM-%';
    ",
    // v5：历史占位。RS485 是总线，同一通道允许挂多台设备；唯一约束在 channel_id+modbus_addr。
    "
    SELECT 1;
    ",
    // v6：告警规则同步。本地规则可编辑，平台规则锁定只读。
    "
    CREATE TABLE IF NOT EXISTS alarm_rule (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        rule_code        TEXT NOT NULL,
        source           TEXT NOT NULL CHECK(source IN ('LOCAL','PLATFORM')),
        name             TEXT NOT NULL,
        level            TEXT NOT NULL DEFAULT 'WARN',
        target_device_sn TEXT,
        point_code       TEXT NOT NULL,
        operator         TEXT NOT NULL DEFAULT '>',
        threshold        REAL NOT NULL DEFAULT 0,
        unit             TEXT NOT NULL DEFAULT '',
        duration_s       INTEGER NOT NULL DEFAULT 60,
        enabled          INTEGER NOT NULL DEFAULT 1,
        updated_ms       INTEGER NOT NULL DEFAULT 0,
        UNIQUE(source, rule_code)
    );
    INSERT OR IGNORE INTO gateway_config(key,value) VALUES('alarm_rule_sync_interval_s','300');
    ",
    // v7：每个采样行保存实际周期，归批后可准确计算云侧完整率。
    "
    ALTER TABLE meter_sample ADD COLUMN sample_interval_s INTEGER NOT NULL DEFAULT 0;
    ",
    // v7：RS485 同总线允许多设备；仅限制同一真实通道内 Modbus 地址不能重复，暂存区不限。
    "
    DROP INDEX IF EXISTS uk_meter_channel_addr;
    CREATE UNIQUE INDEX IF NOT EXISTS uk_meter_channel_addr
      ON meter(channel_id,modbus_addr)
      WHERE channel_id <> '__staging__';
    ",
    // v8：保存完整 Modbus 解码配置，避免平台字节序下发到网关后丢失。
    "
    ALTER TABLE meter_register_map ADD COLUMN byte_order TEXT NOT NULL DEFAULT 'ABCD';
    ",
];

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
        let store = Self {
            conn: Mutex::new(conn),
        };
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

    // ---------- 配置 ----------

    pub fn get_config(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value = conn
            .query_row(
                "SELECT value FROM gateway_config WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }

    pub fn set_config(&self, key: &str, value: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO gateway_config(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn channels(&self) -> StoreResult<Vec<Rs485ChannelRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id,name,port,baud,data_bits,stop_bits,parity,enabled FROM rs485_channel ORDER BY id",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Rs485ChannelRecord {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    port: r.get(2)?,
                    baud: r.get::<_, i64>(3)? as u32,
                    data_bits: r.get::<_, i64>(4)? as u8,
                    stop_bits: r.get::<_, i64>(5)? as u8,
                    parity: r.get(6)?,
                    enabled: r.get::<_, i64>(7)? == 1,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn channel_enabled(&self, channel_id: &str) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT enabled FROM rs485_channel WHERE id=?1",
                params![channel_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|value| value == 1)
            .unwrap_or(false))
    }

    pub fn upsert_channel(&self, row: &Rs485ChannelRecord, now_ms: u64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO rs485_channel(id,name,port,baud,data_bits,stop_bits,parity,enabled,updated_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,port=excluded.port,baud=excluded.baud,
               data_bits=excluded.data_bits,stop_bits=excluded.stop_bits,parity=excluded.parity,
               enabled=excluded.enabled,updated_ms=excluded.updated_ms",
            params![row.id,row.name,row.port,row.baud,row.data_bits,row.stop_bits,row.parity,row.enabled as i64,now_ms as i64],
        )?;
        Ok(())
    }

    pub fn delete_channel_with_meters(&self, channel_id: &str) -> StoreResult<usize> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM rs485_channel WHERE id=?1",
                params![channel_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(StoreError::Validation(format!(
                "missing RS485 channel {channel_id}"
            )));
        }
        let deleted_meters =
            tx.execute("DELETE FROM meter WHERE channel_id=?1", params![channel_id])?;
        tx.execute("DELETE FROM rs485_channel WHERE id=?1", params![channel_id])?;
        tx.commit()?;
        Ok(deleted_meters)
    }

    /// 返回 true 表示告警从非活动状态转为 ACTIVE，需要产生一次北向事件。
    pub fn upsert_alarm(&self, row: &AlarmEventRecord) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let was_active = conn
            .query_row(
                "SELECT status='ACTIVE' FROM alarm_event_local WHERE source=?1 AND source_event_id=?2",
                params![row.source, row.source_event_id],
                |record| record.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            == 1;
        conn.execute(
            "INSERT INTO alarm_event_local(source_event_id,source,meter_id,alarm_type,level,point_code,message,status,first_seen_ms,last_seen_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(source,source_event_id) DO UPDATE SET message=excluded.message,level=excluded.level,
               status=excluded.status,last_seen_ms=excluded.last_seen_ms,occurrence_count=occurrence_count+1",
            params![row.source_event_id,row.source,row.meter_id,row.alarm_type,row.level,row.point_code,
                row.message,row.status,row.first_seen_ms as i64,row.last_seen_ms as i64],
        )?;
        Ok(!was_active && row.status == "ACTIVE")
    }

    pub fn resolve_alarm(
        &self,
        source: &str,
        source_event_id: &str,
        now_ms: u64,
    ) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn.execute(
            "UPDATE alarm_event_local SET status='RECOVERED',last_seen_ms=?3
             WHERE source=?1 AND source_event_id=?2 AND status='ACTIVE'",
            params![source, source_event_id, now_ms as i64],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_alarm_cloud_pending(
        &self,
        source_event_id: &str,
        message_id: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE alarm_event_local SET cloud_message_id=?2,cloud_status='QUEUED',cloud_ack_ms=NULL
             WHERE source='GATEWAY' AND source_event_id=?1",
            params![source_event_id, message_id],
        )?;
        Ok(())
    }

    pub fn mark_alarm_cloud_ack(
        &self,
        source_event_id: &str,
        status: &str,
        ack_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE alarm_event_local SET cloud_status=?2,cloud_ack_ms=?3
             WHERE source='GATEWAY' AND source_event_id=?1",
            params![source_event_id, status, ack_ms as i64],
        )?;
        Ok(())
    }

    /// 平台下行只返回当前活动告警：本轮未返回的平台告警在网关侧标记为已恢复。
    pub fn replace_platform_alarms(
        &self,
        alarms: &[AlarmEventRecord],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE alarm_event_local SET status='RECOVERED',last_seen_ms=?1 WHERE source='PLATFORM' AND status='ACTIVE'",
            params![now_ms as i64],
        )?;
        for row in alarms {
            tx.execute(
                "INSERT INTO alarm_event_local(source_event_id,source,meter_id,alarm_type,level,point_code,message,status,first_seen_ms,last_seen_ms,cloud_status)
                 VALUES(?1,'PLATFORM',?2,?3,?4,?5,?6,'ACTIVE',?7,?8,'SYNCED')
                 ON CONFLICT(source,source_event_id) DO UPDATE SET meter_id=excluded.meter_id,
                   alarm_type=excluded.alarm_type,level=excluded.level,point_code=excluded.point_code,
                   message=excluded.message,status='ACTIVE',last_seen_ms=excluded.last_seen_ms,cloud_status='SYNCED'",
                params![row.source_event_id,row.meter_id,row.alarm_type,row.level,row.point_code,row.message,
                    row.first_seen_ms as i64,row.last_seen_ms as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn recent_alarms(&self, limit: u32) -> StoreResult<Vec<AlarmEventRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT source_event_id,source,meter_id,alarm_type,level,point_code,message,status,
                    first_seen_ms,last_seen_ms,cloud_status,cloud_message_id,cloud_ack_ms
             FROM alarm_event_local ORDER BY last_seen_ms DESC LIMIT ?1",
        )?;
        Ok(stmt
            .query_map(params![limit], |row| {
                Ok(AlarmEventRecord {
                    source_event_id: row.get(0)?,
                    source: row.get(1)?,
                    meter_id: row.get(2)?,
                    alarm_type: row.get(3)?,
                    level: row.get(4)?,
                    point_code: row.get(5)?,
                    message: row.get(6)?,
                    status: row.get(7)?,
                    first_seen_ms: row.get::<_, i64>(8)? as u64,
                    last_seen_ms: row.get::<_, i64>(9)? as u64,
                    cloud_status: row.get(10)?,
                    cloud_message_id: row.get(11)?,
                    cloud_ack_ms: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn acknowledge_active_alarms(&self, ack_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn.execute(
            "UPDATE alarm_event_local SET status='ACKED', last_seen_ms=?1 WHERE status='ACTIVE'",
            params![ack_ms as i64],
        )?;
        Ok(changed)
    }

    pub fn alarm_rules(&self) -> StoreResult<Vec<AlarmRuleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id,rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms
             FROM alarm_rule ORDER BY source DESC, id DESC",
        )?;
        Ok(stmt
            .query_map([], |row| {
                Ok(AlarmRuleRecord {
                    id: row.get(0)?,
                    rule_code: row.get(1)?,
                    source: row.get(2)?,
                    name: row.get(3)?,
                    level: row.get(4)?,
                    target_device_sn: row.get(5)?,
                    point_code: row.get(6)?,
                    operator: row.get(7)?,
                    threshold: row.get(8)?,
                    unit: row.get(9)?,
                    duration_s: row.get::<_, i64>(10)? as u64,
                    enabled: row.get::<_, i64>(11)? == 1,
                    updated_ms: row.get::<_, i64>(12)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn upsert_local_alarm_rule(
        &self,
        id: Option<i64>,
        row: &AlarmRuleRecord,
    ) -> StoreResult<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        if let Some(id) = id {
            let source = conn
                .query_row(
                    "SELECT source FROM alarm_rule WHERE id=?1",
                    params![id],
                    |record| record.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| StoreError::Validation("alarm rule not found".into()))?;
            if source != "LOCAL" {
                return Err(StoreError::Validation(
                    "platform alarm rule is locked".into(),
                ));
            }
            conn.execute(
                "UPDATE alarm_rule SET rule_code=?2,name=?3,level=?4,target_device_sn=?5,point_code=?6,
                    operator=?7,threshold=?8,unit=?9,duration_s=?10,enabled=?11,updated_ms=?12
                 WHERE id=?1 AND source='LOCAL'",
                params![
                    id,
                    row.rule_code,
                    row.name,
                    row.level,
                    row.target_device_sn,
                    row.point_code,
                    row.operator,
                    row.threshold,
                    row.unit,
                    row.duration_s as i64,
                    row.enabled as i64,
                    row.updated_ms as i64
                ],
            )?;
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO alarm_rule(rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms)
             VALUES(?1,'LOCAL',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                row.rule_code,
                row.name,
                row.level,
                row.target_device_sn,
                row.point_code,
                row.operator,
                row.threshold,
                row.unit,
                row.duration_s as i64,
                row.enabled as i64,
                row.updated_ms as i64
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_local_alarm_rule(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn.execute(
            "DELETE FROM alarm_rule WHERE id=?1 AND source='LOCAL'",
            params![id],
        )?;
        if changed == 0 {
            return Err(StoreError::Validation(
                "alarm rule not found or platform rule is locked".into(),
            ));
        }
        Ok(())
    }

    pub fn replace_platform_alarm_rules(
        &self,
        rules: &[AlarmRuleRecord],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM alarm_rule WHERE source='PLATFORM'", [])?;
        for row in rules {
            tx.execute(
                "INSERT INTO alarm_rule(rule_code,source,name,level,target_device_sn,point_code,operator,threshold,unit,duration_s,enabled,updated_ms)
                 VALUES(?1,'PLATFORM',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    row.rule_code,
                    row.name,
                    row.level,
                    row.target_device_sn,
                    row.point_code,
                    row.operator,
                    row.threshold,
                    row.unit,
                    row.duration_s as i64,
                    row.enabled as i64,
                    now_ms as i64
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 原子应用平台下发配置。LOCAL 设备不会被同步删除；新平台设备先进入暂存区。
    pub fn apply_platform_sync(
        &self,
        revision: &str,
        models: &[SyncedThingModel],
        devices: &[SyncedDevice],
        now_ms: u64,
    ) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        for device in devices {
            let has_model = models.iter().any(|model| model.profile == device.profile)
                || tx.query_row(
                    "SELECT COUNT(*) FROM meter_register_map WHERE profile=?1",
                    params![device.profile],
                    |row| row.get::<_, i64>(0),
                )? > 0;
            if !has_model {
                return Err(StoreError::Validation(format!(
                    "device {} references missing thing model {}",
                    device.device_sn, device.profile
                )));
            }
        }
        for model in models {
            for row in &model.points {
                validate_register_row(row)?;
            }
        }
        for model in models {
            tx.execute(
                "INSERT INTO thing_model(profile,name,version,source,platform_model_id,enabled,updated_ms)
                 VALUES(?1,?2,?3,'PLATFORM',?4,1,?5)
                 ON CONFLICT(profile) DO UPDATE SET name=excluded.name,version=excluded.version,
                   source='PLATFORM',platform_model_id=excluded.platform_model_id,enabled=1,updated_ms=excluded.updated_ms",
                params![model.profile,model.name,model.version,model.platform_model_id,now_ms as i64],
            )?;
            tx.execute(
                "DELETE FROM meter_register_map WHERE profile=?1",
                params![model.profile],
            )?;
            for row in &model.points {
                tx.execute(
                    "INSERT INTO meter_register_map(profile,point_code,point_name,unit,func,address,quantity,data_type,byte_order,scale,offset)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![row.profile,row.point_code,row.point_name,row.unit,row.func,row.address,row.quantity,row.data_type,row.byte_order,row.scale,row.offset],
                )?;
            }
        }
        for device in devices {
            let requested_channel = device.channel_id.trim();
            let channel_id = if requested_channel.is_empty() {
                STAGING_CHANNEL_ID.to_string()
            } else {
                requested_channel.to_string()
            };
            let channel_exists = channel_id == STAGING_CHANNEL_ID
                || tx
                    .query_row(
                        "SELECT 1 FROM rs485_channel WHERE id=?1",
                        params![channel_id],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
            let assigned_channel = if channel_exists {
                channel_id
            } else {
                STAGING_CHANNEL_ID.to_string()
            };
            let can_enable =
                channel_exists && assigned_channel != STAGING_CHANNEL_ID && device.enabled;
            tx.execute(
                "INSERT INTO meter(device_sn,device_name,modbus_addr,profile,channel_id,config_source,
                   platform_device_id,model_version,upload_enabled,collect_interval_s,enabled,created_ms)
                 VALUES(?1,?2,?3,?4,?5,'PLATFORM',?6,?7,?10,?8,?10,?9)
                 ON CONFLICT(device_sn) DO UPDATE SET device_name=excluded.device_name,modbus_addr=excluded.modbus_addr,
                   profile=excluded.profile,channel_id=excluded.channel_id,config_source='PLATFORM',
                   platform_device_id=excluded.platform_device_id,model_version=excluded.model_version,
                   upload_enabled=?10,
                   collect_interval_s=excluded.collect_interval_s,
                   enabled=?10",
                params![device.device_sn,device.device_name,device.modbus_addr,device.profile,assigned_channel,
                    device.platform_device_id,device.model_version,device.collect_interval_s as i64,now_ms as i64,
                    can_enable as i64],
            )?;
        }
        tx.execute(
            "UPDATE config_sync_state SET desired_revision=?1,applied_revision=?1,last_sync_ms=?2,
               last_status='APPLIED',last_error=NULL WHERE id=1",
            params![revision, now_ms as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn bind_meter_to_channel(&self, meter_id: i64, channel_id: &str) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let channel_enabled: Option<i64> = tx
            .query_row(
                "SELECT enabled FROM rs485_channel WHERE id=?1",
                params![channel_id],
                |row| row.get(0),
            )
            .optional()?;
        if channel_enabled.is_none() {
            return Err(StoreError::Validation(format!(
                "missing RS485 channel {channel_id}"
            )));
        }
        if channel_enabled != Some(1) {
            return Err(StoreError::Validation(format!(
                "disabled RS485 channel {channel_id}"
            )));
        }
        let source: String = tx
            .query_row(
                "SELECT config_source FROM meter WHERE id=?1",
                params![meter_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Validation("device not found".into()))?;
        let modbus_addr: i64 = tx
            .query_row(
                "SELECT modbus_addr FROM meter WHERE id=?1",
                params![meter_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::Validation("device not found".into()))?;
        let duplicated: Option<String> = tx
            .query_row(
                "SELECT device_sn FROM meter WHERE channel_id=?1 AND modbus_addr=?2 AND id<>?3",
                params![channel_id, modbus_addr, meter_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(device_sn) = duplicated {
            return Err(StoreError::Validation(format!(
                "RS485 channel {channel_id} already has Modbus address {modbus_addr} on {device_sn}"
            )));
        }
        tx.execute(
            "UPDATE meter SET channel_id=?2,enabled=1,
               upload_enabled=CASE WHEN ?3='PLATFORM' THEN 1 ELSE upload_enabled END
             WHERE id=?1",
            params![meter_id, channel_id, source],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn thing_models(&self) -> StoreResult<Vec<ThingModelSummary>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT p.profile,COALESCE(t.name,p.profile),COALESCE(t.version,'local-v1'),
                    COALESCE(t.source,'LOCAL'),COUNT(DISTINCT p.point_code),COUNT(DISTINCT m.id)
             FROM meter_register_map p LEFT JOIN thing_model t ON t.profile=p.profile
             LEFT JOIN meter m ON m.profile=p.profile GROUP BY p.profile,t.name,t.version,t.source ORDER BY p.profile",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(ThingModelSummary {
                    profile: r.get(0)?,
                    name: r.get(1)?,
                    version: r.get(2)?,
                    source: r.get(3)?,
                    point_count: r.get::<_, i64>(4)? as u32,
                    device_count: r.get::<_, i64>(5)? as u32,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_thing_model(&self, profile: &str) -> StoreResult<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let used: i64 = conn.query_row(
            "SELECT COUNT(*) FROM meter WHERE profile=?1",
            params![profile],
            |r| r.get(0),
        )?;
        if used > 0 {
            return Err(StoreError::Validation(format!(
                "thing model {profile} is used by {used} device(s)"
            )));
        }
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM meter_register_map WHERE profile=?1",
            params![profile],
        )?;
        tx.execute("DELETE FROM thing_model WHERE profile=?1", params![profile])?;
        tx.commit()?;
        Ok(())
    }

    pub fn register_local_thing_model(
        &self,
        profile: &str,
        name: &str,
        now_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO thing_model(profile,name,version,source,enabled,updated_ms)
             VALUES(?1,?2,'local-v1','LOCAL',1,?3)
             ON CONFLICT(profile) DO NOTHING",
            params![profile, name, now_ms as i64],
        )?;
        Ok(())
    }

    pub fn record_sync_failure(
        &self,
        revision: Option<&str>,
        error: &str,
        now_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE config_sync_state SET desired_revision=COALESCE(?1,desired_revision),last_sync_ms=?2,
               last_status='FAILED',last_error=?3 WHERE id=1",
            params![revision,now_ms as i64,error],
        )?;
        Ok(())
    }

    // ---------- 电表档案 ----------

    pub fn insert_meter(&self, input: &MeterInput, now_ms: u64) -> StoreResult<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO meter(device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                               model_version, upload_enabled, collect_interval_s, enabled, created_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, 'LOCAL', 'local-v1', ?6, ?7, ?8, ?9)",
            params![
                input.device_sn,
                input.device_name,
                input.modbus_addr,
                input.profile,
                input.channel_id,
                input.upload_enabled as i64,
                input.collect_interval_s as i64,
                input.enabled as i64,
                now_ms as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_meter(&self, id: i64, input: &MeterInput) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn.execute(
            "UPDATE meter SET device_sn=?2, device_name=?3, modbus_addr=?4, profile=?5, channel_id=?6,
                              upload_enabled=?7, collect_interval_s=?8, enabled=?9
             WHERE id=?1",
            params![
                id,
                input.device_sn,
                input.device_name,
                input.modbus_addr,
                input.profile,
                input.channel_id,
                input.upload_enabled as i64,
                input.collect_interval_s as i64,
                input.enabled as i64
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    pub fn delete_meter(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute("DELETE FROM meter WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn meters(&self) -> StoreResult<Vec<MeterRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                    platform_device_id, model_version, upload_enabled, collect_interval_s, enabled, created_ms
             FROM meter ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MeterRecord {
                    id: r.get(0)?,
                    device_sn: r.get(1)?,
                    device_name: r.get(2)?,
                    modbus_addr: r.get::<_, i64>(3)? as u8,
                    profile: r.get(4)?,
                    channel_id: r.get(5)?,
                    config_source: r.get(6)?,
                    platform_device_id: r.get(7)?,
                    model_version: r.get(8)?,
                    upload_enabled: r.get::<_, i64>(9)? == 1,
                    collect_interval_s: r.get::<_, i64>(10)? as u64,
                    enabled: r.get::<_, i64>(11)? == 1,
                    created_ms: r.get::<_, i64>(12)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn meter_by_sn(&self, device_sn: &str) -> StoreResult<Option<MeterRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let record = conn
            .query_row(
                "SELECT id, device_sn, device_name, modbus_addr, profile, channel_id, config_source,
                        platform_device_id, model_version, upload_enabled, collect_interval_s, enabled, created_ms
                 FROM meter WHERE device_sn=?1",
                params![device_sn],
                |r| {
                    Ok(MeterRecord {
                        id: r.get(0)?,
                        device_sn: r.get(1)?,
                        device_name: r.get(2)?,
                        modbus_addr: r.get::<_, i64>(3)? as u8,
                        profile: r.get(4)?,
                        channel_id: r.get(5)?,
                        config_source: r.get(6)?,
                        platform_device_id: r.get(7)?,
                        model_version: r.get(8)?,
                        upload_enabled: r.get::<_, i64>(9)? == 1,
                        collect_interval_s: r.get::<_, i64>(10)? as u64,
                        enabled: r.get::<_, i64>(11)? == 1,
                        created_ms: r.get::<_, i64>(12)? as u64,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    // ---------- 寄存器档案 ----------

    /// 以覆盖方式导入一个档案（事务内先删该 profile 再插入全部行）。
    pub fn replace_register_map(&self, profile: &str, rows: &[RegisterMapRow]) -> StoreResult<()> {
        for row in rows {
            validate_register_row(row)?;
        }
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM meter_register_map WHERE profile=?1",
            params![profile],
        )?;
        for row in rows {
            tx.execute(
                "INSERT INTO meter_register_map(profile, point_code, point_name, unit, func, address, quantity, data_type, byte_order, scale, offset)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    row.profile,
                    row.point_code,
                    row.point_name,
                    row.unit,
                    row.func,
                    row.address,
                    row.quantity,
                    row.data_type,
                    row.byte_order,
                    row.scale,
                    row.offset
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn register_map(&self, profile: &str) -> StoreResult<Vec<RegisterMapRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT profile, point_code, point_name, unit, func, address, quantity, data_type, byte_order, scale, offset
             FROM meter_register_map WHERE profile=?1 ORDER BY address",
        )?;
        let rows = stmt
            .query_map(params![profile], |r| {
                Ok(RegisterMapRow {
                    profile: r.get(0)?,
                    point_code: r.get(1)?,
                    point_name: r.get(2)?,
                    unit: r.get(3)?,
                    func: r.get::<_, i64>(4)? as u8,
                    address: r.get::<_, i64>(5)? as u16,
                    quantity: r.get::<_, i64>(6)? as u16,
                    data_type: r.get(7)?,
                    byte_order: r.get(8)?,
                    scale: r.get(9)?,
                    offset: r.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------- outbox ----------

    /// 入队。message_id 冲突（进程内重试/重复入队）静默忽略，返回是否真正插入。
    pub fn enqueue(&self, item: &NewOutbox) -> StoreResult<bool> {
        self.enqueue_with_status(item, "pending")
    }

    /// 以指定状态入队（'pending' 或 'deferred'：时钟不可信期间暂存、待补时间戳的行）。
    pub fn enqueue_with_status(&self, item: &NewOutbox, status: &str) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let changed = conn.execute(
            "INSERT OR IGNORE INTO outbox_message(message_id, topic, payload, created_ms, status)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                item.message_id,
                item.topic,
                item.payload,
                item.created_ms as i64,
                status
            ],
        )?;
        Ok(changed == 1)
    }

    /// 原子地创建一个上报批次并标记其包含的历史样本。进程在任意一步退出都不会丢样本。
    pub fn enqueue_sample_batch(
        &self,
        item: &NewOutbox,
        status: &str,
        sample_ids: &[i64],
    ) -> StoreResult<bool> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "INSERT OR IGNORE INTO outbox_message(message_id,topic,payload,created_ms,status)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                item.message_id,
                item.topic,
                item.payload,
                item.created_ms as i64,
                status
            ],
        )?;
        if changed == 0 {
            tx.rollback()?;
            return Ok(false);
        }
        let outbox_id = tx.last_insert_rowid();
        for sample_id in sample_ids {
            tx.execute(
                "UPDATE meter_sample SET outbox_id=?2 WHERE id=?1 AND outbox_id IS NULL",
                params![sample_id, outbox_id],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// 取延迟暂存行（按入队顺序）。
    pub fn deferred_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, message_id, topic, payload, created_ms, attempts
             FROM outbox_message WHERE status='deferred' ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(OutboxItem {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    topic: r.get(2)?,
                    payload: r.get(3)?,
                    created_ms: r.get::<_, i64>(4)? as u64,
                    attempts: r.get::<_, u32>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 把延迟行定稿为待发布行（补时间戳与正式 messageId）。
    pub fn finalize_outbox(
        &self,
        id: i64,
        message_id: &str,
        payload: &[u8],
        created_ms: u64,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET message_id=?2, payload=?3, created_ms=?4, status='pending'
             WHERE id=?1",
            params![id, message_id, payload, created_ms as i64],
        )?;
        Ok(())
    }

    /// 按入队顺序取待发布报文。
    pub fn pending_outbox(&self, limit: u32) -> StoreResult<Vec<OutboxItem>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, message_id, topic, payload, created_ms, attempts
             FROM outbox_message WHERE status='pending' ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(OutboxItem {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    topic: r.get(2)?,
                    payload: r.get(3)?,
                    created_ms: r.get::<_, i64>(4)? as u64,
                    attempts: r.get::<_, u32>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_sent(&self, id: i64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET status='sent' WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    /// 记录一次失败尝试（保持 pending，供补偿重试），返回累计尝试次数。
    pub fn mark_attempt_failed(&self, id: i64, error: &str) -> StoreResult<u32> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE outbox_message SET attempts=attempts+1, last_error=?2 WHERE id=?1",
            params![id, error],
        )?;
        let attempts: u32 = conn.query_row(
            "SELECT attempts FROM outbox_message WHERE id=?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(attempts)
    }

    pub fn outbox_pending_count(&self) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox_message WHERE status='pending'",
            [],
            |r| r.get(0),
        )?;
        Ok(count as u64)
    }

    pub fn outbox_sent_count(&self) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox_message WHERE status='sent'",
            [],
            |r| r.get(0),
        )?;
        Ok(count as u64)
    }

    /// 清理 cutoff 之前的已发布行（防无限增长）。
    pub fn cleanup_sent_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let deleted = conn.execute(
            "DELETE FROM outbox_message WHERE status='sent' AND created_ms < ?1",
            params![cutoff_ms as i64],
        )?;
        Ok(deleted)
    }

    // ---------- 读数 / 电能日账 ----------

    pub fn save_reading(&self, record: &ReadingRecord, updated_ms: u64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO meter_reading(meter_id, snapshot, read_ms, quality, updated_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(meter_id) DO UPDATE SET
                snapshot=excluded.snapshot, read_ms=excluded.read_ms,
                quality=excluded.quality, updated_ms=excluded.updated_ms",
            params![
                record.meter_id,
                record.snapshot_json,
                record.read_ms as i64,
                record.quality,
                updated_ms as i64
            ],
        )?;
        Ok(())
    }

    /// 保存最新快照的同时追加不可覆盖的历史样本。
    pub fn save_sample(&self, record: &ReadingRecord, updated_ms: u64) -> StoreResult<i64> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO meter_reading(meter_id,snapshot,read_ms,quality,updated_ms)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(meter_id) DO UPDATE SET snapshot=excluded.snapshot,read_ms=excluded.read_ms,
               quality=excluded.quality,updated_ms=excluded.updated_ms",
            params![record.meter_id, record.snapshot_json, record.read_ms as i64, record.quality, updated_ms as i64],
        )?;
        tx.execute(
            "INSERT INTO meter_sample(meter_id,read_ms,sample_interval_s,quality,points_json,created_ms)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                record.meter_id,
                record.read_ms as i64,
                record.sample_interval_s as i64,
                record.quality,
                record.snapshot_json,
                updated_ms as i64
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn unbatched_samples(&self, limit: u32) -> StoreResult<Vec<SampleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT s.id,s.meter_id,m.device_sn,m.modbus_addr,m.channel_id,m.profile,m.model_version,
                    (SELECT applied_revision FROM config_sync_state WHERE id=1),
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             WHERE s.outbox_id IS NULL AND m.upload_enabled=1
             ORDER BY s.id LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(SampleRecord {
                    id: r.get(0)?,
                    meter_id: r.get(1)?,
                    device_sn: r.get(2)?,
                    modbus_addr: r.get::<_, i64>(3)? as u8,
                    channel_id: r.get(4)?,
                    profile: r.get(5)?,
                    model_version: r.get(6)?,
                    config_revision: r.get(7)?,
                    read_ms: r.get::<_, i64>(8)? as u64,
                    sample_interval_s: r.get::<_, i64>(9)? as u64,
                    quality: r.get::<_, i64>(10)? as u32,
                    points_json: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn sample_history_since(&self, from_ms: u64, limit: u32) -> StoreResult<Vec<SampleRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT s.id,s.meter_id,m.device_sn,m.modbus_addr,m.channel_id,m.profile,m.model_version,
                    (SELECT applied_revision FROM config_sync_state WHERE id=1),
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             WHERE s.read_ms>=?1 ORDER BY s.read_ms,s.id LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![from_ms as i64, limit], |r| {
                Ok(SampleRecord {
                    id: r.get(0)?,
                    meter_id: r.get(1)?,
                    device_sn: r.get(2)?,
                    modbus_addr: r.get::<_, i64>(3)? as u8,
                    channel_id: r.get(4)?,
                    profile: r.get(5)?,
                    model_version: r.get(6)?,
                    config_revision: r.get(7)?,
                    read_ms: r.get::<_, i64>(8)? as u64,
                    sample_interval_s: r.get::<_, i64>(9)? as u64,
                    quality: r.get::<_, i64>(10)? as u32,
                    points_json: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn recent_samples_page(
        &self,
        page_num: u32,
        page_size: u32,
    ) -> StoreResult<(Vec<SampleRecord>, u64)> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM meter_sample", [], |row| row.get(0))?;
        let page_num = page_num.max(1);
        let page_size = page_size.clamp(1, 500);
        let offset = (page_num - 1) * page_size;
        let mut stmt = conn.prepare(
            "SELECT s.id,s.meter_id,m.device_sn,m.modbus_addr,m.channel_id,m.profile,m.model_version,
                    (SELECT applied_revision FROM config_sync_state WHERE id=1),
                    s.read_ms,s.sample_interval_s,s.quality,s.points_json
             FROM meter_sample s JOIN meter m ON m.id=s.meter_id
             ORDER BY s.read_ms DESC,s.id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map(params![page_size, offset], |r| {
                Ok(SampleRecord {
                    id: r.get(0)?,
                    meter_id: r.get(1)?,
                    device_sn: r.get(2)?,
                    modbus_addr: r.get::<_, i64>(3)? as u8,
                    channel_id: r.get(4)?,
                    profile: r.get(5)?,
                    model_version: r.get(6)?,
                    config_revision: r.get(7)?,
                    read_ms: r.get::<_, i64>(8)? as u64,
                    sample_interval_s: r.get::<_, i64>(9)? as u64,
                    quality: r.get::<_, i64>(10)? as u32,
                    points_json: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, total as u64))
    }

    pub fn cleanup_samples_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM meter_sample WHERE read_ms<?1 AND outbox_id IN
             (SELECT id FROM outbox_message WHERE status='sent')",
            params![cutoff_ms as i64],
        )?)
    }

    /// LOCAL 设备默认不进入正式上报，但仍保留 7 天本地分析数据。
    pub fn cleanup_local_samples_before(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        Ok(conn.execute(
            "DELETE FROM meter_sample WHERE read_ms<?1 AND outbox_id IS NULL AND meter_id IN
             (SELECT id FROM meter WHERE upload_enabled=0)",
            params![cutoff_ms as i64],
        )?)
    }

    pub fn reading(&self, meter_id: i64) -> StoreResult<Option<ReadingRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let record = conn
            .query_row(
                "SELECT meter_id, snapshot, read_ms, quality FROM meter_reading WHERE meter_id=?1",
                params![meter_id],
                |r| {
                    Ok(ReadingRecord {
                        meter_id: r.get(0)?,
                        snapshot_json: r.get(1)?,
                        read_ms: r.get::<_, i64>(2)? as u64,
                        sample_interval_s: 0,
                        quality: r.get::<_, i64>(3)? as u32,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    /// 累加某表某日增量。
    pub fn add_energy(&self, meter_id: i64, day: &str, delta_kwh: f64) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO energy_daily(meter_id, day, delta_kwh) VALUES(?1, ?2, ?3)
             ON CONFLICT(meter_id, day) DO UPDATE SET delta_kwh = delta_kwh + excluded.delta_kwh",
            params![meter_id, day, delta_kwh],
        )?;
        Ok(())
    }

    pub fn energy_of(&self, meter_id: i64, day: &str) -> StoreResult<f64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value = conn
            .query_row(
                "SELECT delta_kwh FROM energy_daily WHERE meter_id=?1 AND day=?2",
                params![meter_id, day],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.unwrap_or(0.0))
    }

    pub fn energy_sum(&self, day: &str) -> StoreResult<f64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let value: f64 = conn.query_row(
            "SELECT COALESCE(SUM(delta_kwh), 0) FROM energy_daily WHERE day=?1",
            params![day],
            |r| r.get(0),
        )?;
        Ok(value)
    }

    // ---------- 指令 / 事件日志 ----------

    pub fn record_command(&self, row: &CommandLogRow) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO command_log(command_id, command_type, target_sn, payload, received_ms, result_status, responded_ms, message)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(command_id) DO UPDATE SET
                result_status=excluded.result_status,
                responded_ms=excluded.responded_ms,
                message=excluded.message",
            params![
                row.command_id,
                row.command_type,
                row.target_sn,
                row.payload_json,
                row.received_ms as i64,
                row.result_status,
                row.responded_ms as i64,
                row.message
            ],
        )?;
        Ok(())
    }

    pub fn recent_commands(&self, limit: u32) -> StoreResult<Vec<CommandLogRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT command_id, command_type, target_sn, payload, received_ms, result_status, responded_ms, message
             FROM command_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(CommandLogRow {
                    command_id: r.get(0)?,
                    command_type: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    target_sn: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    payload_json: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    received_ms: r.get::<_, i64>(4)? as u64,
                    result_status: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    responded_ms: r.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                    message: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn log_event(&self, row: &EventRow) -> StoreResult<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO event_log(ts_ms, level, source, message) VALUES(?1, ?2, ?3, ?4)",
            params![row.ts_ms as i64, row.level, row.source, row.message],
        )?;
        Ok(())
    }

    pub fn recent_events(&self, limit: u32) -> StoreResult<Vec<EventRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT ts_ms, level, source, message FROM event_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(EventRow {
                    ts_ms: r.get::<_, i64>(0)? as u64,
                    level: r.get(1)?,
                    source: r.get(2)?,
                    message: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 事件日志裁剪：只保留最近 limit 行。
    pub fn trim_events(&self, limit: u32) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let deleted = conn.execute(
            "DELETE FROM event_log WHERE id <= (SELECT id FROM event_log ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![limit],
        )?;
        Ok(deleted)
    }
}

fn validate_register_row(row: &RegisterMapRow) -> StoreResult<()> {
    if row.point_code.trim().is_empty() {
        return Err(StoreError::Validation("point_code is required".into()));
    }
    if !matches!(row.func, 3 | 4) {
        return Err(StoreError::Validation(format!(
            "point {} uses unsupported Modbus function code {}",
            row.point_code, row.func
        )));
    }
    let data_type = row.data_type.trim().to_ascii_lowercase();
    let expected_quantity = match data_type.as_str() {
        "u16" | "i16" => 1,
        "u32" | "i32" | "f32" | "float32" => 2,
        _ => {
            return Err(StoreError::Validation(format!(
                "point {} uses unsupported data type {}",
                row.point_code, row.data_type
            )));
        }
    };
    if row.quantity != expected_quantity {
        return Err(StoreError::Validation(format!(
            "point {} quantity must be {} for {}",
            row.point_code, expected_quantity, row.data_type
        )));
    }
    let end = row.address as u32 + row.quantity as u32;
    if end > 65_536 {
        return Err(StoreError::Validation(format!(
            "point {} register address overflows Modbus range",
            row.point_code
        )));
    }
    let order = row.byte_order.trim().to_ascii_uppercase();
    let valid_order = match row.quantity {
        1 => matches!(order.as_str(), "" | "AB" | "BA" | "ABCD"),
        2 => matches!(order.as_str(), "" | "ABCD" | "BADC" | "CDAB" | "DCBA"),
        _ => false,
    };
    if !valid_order {
        return Err(StoreError::Validation(format!(
            "point {} byte_order {} does not match quantity {}",
            row.point_code, row.byte_order, row.quantity
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(name: &str) -> Store {
        let path =
            std::env::temp_dir().join(format!("park-gateway-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Store::open(&path).expect("open test store")
    }

    fn meter(sn: &str, channel_id: &str, modbus_addr: u8) -> MeterInput {
        MeterInput {
            device_sn: sn.to_string(),
            device_name: sn.to_string(),
            modbus_addr,
            profile: "PD666-3S3".to_string(),
            channel_id: channel_id.to_string(),
            upload_enabled: false,
            collect_interval_s: 60,
            enabled: true,
        }
    }

    #[test]
    fn rs485_channel_allows_many_devices_but_unique_addresses() {
        let store = test_store("rs485-bus");

        store
            .insert_meter(&meter("METER-1", "rs485-1", 1), 1)
            .expect("first address should be accepted");
        store
            .insert_meter(&meter("METER-2", "rs485-1", 2), 2)
            .expect("second address on same bus should be accepted");
        assert!(
            store
                .insert_meter(&meter("METER-3", "rs485-1", 1), 3)
                .is_err(),
            "same channel and same Modbus address must be rejected"
        );
    }

    #[test]
    fn staging_devices_do_not_claim_modbus_addresses() {
        let store = test_store("staging");

        store
            .insert_meter(&meter("STAGED-1", STAGING_CHANNEL_ID, 1), 1)
            .expect("first staged device should be accepted");
        store
            .insert_meter(&meter("STAGED-2", STAGING_CHANNEL_ID, 1), 2)
            .expect("staging does not participate in bus address uniqueness");
    }
}
