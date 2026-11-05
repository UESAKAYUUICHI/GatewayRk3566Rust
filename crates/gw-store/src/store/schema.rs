pub(super) const MIGRATIONS: &[&str] = &[
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
    "
    ALTER TABLE alarm_event_local ADD COLUMN cloud_message_id TEXT;
    ALTER TABLE alarm_event_local ADD COLUMN cloud_status TEXT NOT NULL DEFAULT 'LOCAL';
    ALTER TABLE alarm_event_local ADD COLUMN cloud_ack_ms INTEGER;
    ",
    "
    DELETE FROM meter WHERE device_sn LIKE 'SIM-%';
    ",
    "
    SELECT 1;
    ",
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
    "
    ALTER TABLE meter_sample ADD COLUMN sample_interval_s INTEGER NOT NULL DEFAULT 0;
    ",
    "
    DROP INDEX IF EXISTS uk_meter_channel_addr;
    CREATE UNIQUE INDEX IF NOT EXISTS uk_meter_channel_addr
      ON meter(channel_id,modbus_addr)
      WHERE channel_id <> '__staging__';
    ",
    "
    ALTER TABLE meter_register_map ADD COLUMN byte_order TEXT NOT NULL DEFAULT 'ABCD';
    ",
    "
    ALTER TABLE rs485_channel ADD COLUMN protocol TEXT NOT NULL DEFAULT 'MODBUS_RTU';
    ALTER TABLE rs485_channel ADD COLUMN timeout_ms INTEGER NOT NULL DEFAULT 1000;
    ALTER TABLE rs485_channel ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 2;
    ALTER TABLE rs485_channel ADD COLUMN poll_interval_s INTEGER NOT NULL DEFAULT 300;
    ALTER TABLE rs485_channel ADD COLUMN config_source TEXT NOT NULL DEFAULT 'LOCAL';
    ",
    "
    ALTER TABLE meter_register_map ADD COLUMN field_offset INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE meter_register_map ADD COLUMN field_quantity INTEGER NOT NULL DEFAULT 1;
    ALTER TABLE meter_register_map ADD COLUMN bit_offset INTEGER;
    ALTER TABLE meter_register_map ADD COLUMN bit_length INTEGER;
    ",
    "
    CREATE TABLE protocol_command (
        profile          TEXT NOT NULL,
        command_code     TEXT NOT NULL,
        command_name     TEXT NOT NULL,
        function_code    INTEGER NOT NULL,
        register_address INTEGER NOT NULL,
        encode_type      TEXT NOT NULL,
        fixed_value      INTEGER,
        parameter_json   TEXT,
        PRIMARY KEY(profile, command_code)
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS meter_weekly_summary (
        meter_id          INTEGER NOT NULL REFERENCES meter(id) ON DELETE CASCADE,
        week_start_ms     INTEGER NOT NULL,
        week_end_ms       INTEGER NOT NULL,
        sample_count      INTEGER NOT NULL DEFAULT 0,
        normal_count      INTEGER NOT NULL DEFAULT 0,
        first_read_ms     INTEGER NOT NULL DEFAULT 0,
        last_read_ms      INTEGER NOT NULL DEFAULT 0,
        first_energy_kwh  REAL,
        last_energy_kwh   REAL,
        delta_kwh         REAL NOT NULL DEFAULT 0,
        avg_power_kw      REAL,
        min_voltage_v     REAL,
        max_voltage_v     REAL,
        created_ms        INTEGER NOT NULL,
        PRIMARY KEY(meter_id, week_start_ms)
    );
    CREATE INDEX IF NOT EXISTS idx_weekly_summary_time
      ON meter_weekly_summary(week_start_ms, meter_id);
    INSERT OR IGNORE INTO gateway_config(key,value) VALUES('sample_retention_days','7');
    INSERT OR IGNORE INTO gateway_config(key,value) VALUES('weekly_archive_last_start_ms','0');
    ",
    "
    CREATE TABLE IF NOT EXISTS alarm_protocol (
        protocol_id   INTEGER PRIMARY KEY,
        protocol_key  TEXT NOT NULL UNIQUE,
        protocol_name TEXT NOT NULL,
        version_no    INTEGER NOT NULL DEFAULT 1,
        enabled       INTEGER NOT NULL DEFAULT 1,
        updated_ms    INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS alarm_protocol_point (
        point_id       INTEGER PRIMARY KEY,
        protocol_id    INTEGER NOT NULL REFERENCES alarm_protocol(protocol_id) ON DELETE CASCADE,
        point_code     TEXT NOT NULL,
        point_name     TEXT NOT NULL,
        alarm_type     TEXT NOT NULL,
        level          TEXT NOT NULL,
        operator       TEXT NOT NULL,
        threshold_value REAL,
        threshold_min   REAL,
        threshold_max   REAL,
        recovery_threshold_value REAL,
        duration_s     INTEGER NOT NULL DEFAULT 0,
        enabled        INTEGER NOT NULL DEFAULT 1,
        UNIQUE(protocol_id, point_code)
    );
    CREATE TABLE IF NOT EXISTS alarm_protocol_binding (
        protocol_id INTEGER NOT NULL REFERENCES alarm_protocol(protocol_id) ON DELETE CASCADE,
        device_sn   TEXT NOT NULL,
        enabled     INTEGER NOT NULL DEFAULT 1,
        PRIMARY KEY(protocol_id, device_sn)
    );
    INSERT OR IGNORE INTO gateway_config(key,value) VALUES('alarm_protocol_sync_interval_s','300');
    ",
    "ALTER TABLE meter_sample ADD COLUMN metadata_json TEXT;",
];
