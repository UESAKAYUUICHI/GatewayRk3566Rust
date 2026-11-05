use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    fn test_store(name: &str) -> Store {
        let path = std::env::temp_dir()
            .join(format!("park-gateway-{name}-{}.db", std::process::id()));
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
            store.insert_meter(& meter("METER-3", "rs485-1", 1), 3).is_err(),
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
    fn synced_channel(id: &str, port: &str) -> SyncedChannel {
        SyncedChannel {
            channel_id: id.to_string(),
            channel_name: id.to_string(),
            protocol: "MODBUS_RTU".to_string(),
            serial_port: port.to_string(),
            baud_rate: 19_200,
            data_bits: 8,
            stop_bits: 1,
            parity: "E".to_string(),
            timeout_ms: 1_500,
            retry_count: 3,
            poll_interval_s: 30,
            enabled: true,
        }
    }
    #[test]
    fn platform_sync_persists_channel_runtime_settings() {
        let store = test_store("platform-channel");
        store
            .apply_platform_sync(
                "rev-1",
                &[synced_channel("rs485-9", "COM20")],
                &[],
                &[],
                10,
            )
            .expect("platform channel should be staged");
        let channel = store
            .channels()
            .expect("list channels")
            .into_iter()
            .find(|item| item.id == "rs485-9")
            .expect("synced channel exists");
        assert_eq!(channel.port, "COM20");
        assert_eq!(channel.baud, 19_200);
        assert_eq!(channel.parity, "EVEN");
        assert_eq!(channel.timeout_ms, 1_500);
        assert_eq!(channel.retry_count, 3);
        assert_eq!(channel.poll_interval_s, 30);
        assert_eq!(channel.config_source, "PLATFORM");
    }
    #[test]
    fn platform_sync_rejects_unsupported_protocol_and_duplicate_port() {
        let store = test_store("platform-channel-invalid");
        let mut tcp = synced_channel("tcp-1", "127.0.0.1:502");
        tcp.protocol = "MODBUS_TCP".to_string();
        assert!(store.apply_platform_sync("rev-1", & [tcp], & [], & [], 10).is_err());
        assert!(
            store.apply_platform_sync("rev-2", & [synced_channel("rs485-1", "COM20"),
            synced_channel("rs485-2", "com20"),], & [], & [], 20,).is_err()
        );
    }
    #[test]
    fn quarantine_retains_payload_and_is_not_a_successful_delivery() {
        let store = test_store("outbox-quarantine");
        let item = NewOutbox { message_id: "broken".into(), topic: "data".into(),
            payload: b"invalid-json".to_vec(), created_ms: 1 };
        store.enqueue_with_status(&item, "deferred").unwrap();
        let id = store.deferred_outbox(1).unwrap()[0].id;
        store.quarantine_outbox(id, "invalid payload").unwrap();
        assert_eq!(store.outbox_sent_count().unwrap(), 0);
        assert!(store.pending_outbox(10).unwrap().is_empty());
        store.cleanup_sent_before(100).unwrap();
        let rows = store.recent_outbox(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "quarantined");
        assert_eq!(rows[0].payload, b"invalid-json");
    }
    #[test]
    fn pending_samples_keep_the_configuration_active_at_collection() {
        let store = test_store("sample-metadata");
        let mut input = meter("METER-1", "rs485-1", 1);
        input.upload_enabled = true;
        let id = store.insert_meter(&input, 1).unwrap();
        store.record_sync_applied("revision-before", 1).unwrap();
        store.save_sample(&ReadingRecord { meter_id:id,snapshot_json:"[]".into(),
            read_ms:10,sample_interval_s:60,quality:0 },10).unwrap();
        input.profile = "changed-profile".into();
        input.modbus_addr = 2;
        store.update_meter(id,&input).unwrap();
        store.record_sync_applied("revision-after",20).unwrap();
        let pending = store.unbatched_samples(10).unwrap();
        assert_eq!(pending[0].profile,"PD666-3S3");
        assert_eq!(pending[0].modbus_addr,1);
        assert_eq!(pending[0].config_revision.as_deref(),Some("revision-before"));
    }
}
