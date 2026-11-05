use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct BlockTransport(Arc<AtomicUsize>);
    #[async_trait]
    impl MeterTransport for BlockTransport {
        async fn read_registers(
            &mut self,
            _: u8,
            _: u8,
            _: u16,
            _: u16,
        ) -> Result<Vec<u16>, gw_collector::TransportError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![0x8001])
        }
    }
    fn bit_row(code: &str, bit: u8) -> gw_store::RegisterMapRow {
        gw_store::RegisterMapRow {
            profile: "LIGHTING".into(),
            point_code: code.into(),
            point_name: code.into(),
            unit: String::new(),
            func: 3,
            address: 0x007A,
            quantity: 1,
            field_offset: 0,
            field_quantity: 1,
            bit_offset: Some(bit),
            bit_length: Some(1),
            data_type: "boolean".into(),
            byte_order: "AB".into(),
            scale: 1.0,
            offset: 0.0,
        }
    }
    #[tokio::test]
    async fn shared_status_block_is_read_once_and_split_into_bits() {
        let calls = Arc::new(AtomicUsize::new(0));
        let transport = Mutex::new(BlockTransport(calls.clone()));
        let meter = MeterRecord {
            id: 1,
            device_sn: "LIGHT-001".into(),
            device_name: "light".into(),
            modbus_addr: 7,
            profile: "LIGHTING".into(),
            channel_id: "rs485-1".into(),
            config_source: "PLATFORM".into(),
            platform_device_id: Some(1),
            model_version: "V1".into(),
            upload_enabled: true,
            collect_interval_s: 60,
            enabled: true,
            created_ms: 0,
        };
        let points = poll_once(
                &transport,
                &meter,
                &[bit_row("LOOP_1_STATUS", 15), bit_row("LOOP_16_STATUS", 0)],
            )
            .await
            .expect("block should decode");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            points, vec![("LOOP_1_STATUS".into(), 1.0), ("LOOP_16_STATUS".into(), 1.0)]
        );
    }
}
