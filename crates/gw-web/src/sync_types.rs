use super::*;
#[derive(Debug, Deserialize)]
pub(super) struct PlatformEnvelope<T> {
    pub(super) code: i32,
    pub(super) message: String,
    pub(super) data: Option<T>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncPayload {
    pub(super) desired_revision: String,
    #[serde(default)]
    pub(super) config_checksum: Option<String>,
    pub(super) channels: Vec<SyncChannelDto>,
    pub(super) devices: Vec<SyncDeviceDto>,
    pub(super) models: Vec<SyncModelDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncChannelDto {
    pub(super) channel_id: String,
    pub(super) channel_name: String,
    pub(super) protocol: String,
    pub(super) serial_port: String,
    pub(super) baud_rate: u32,
    pub(super) data_bits: u8,
    pub(super) stop_bits: u8,
    pub(super) parity: String,
    pub(super) timeout_ms: u64,
    pub(super) retry_count: u32,
    pub(super) poll_interval_seconds: u64,
    #[serde(deserialize_with = "boolish")]
    pub(super) enabled: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncDeviceDto {
    pub(super) platform_device_id: i64,
    pub(super) device_sn: String,
    pub(super) device_name: String,
    pub(super) modbus_addr: u8,
    pub(super) channel_id: String,
    pub(super) collect_interval_s: u64,
    #[serde(deserialize_with = "boolish")]
    pub(super) enabled: bool,
    pub(super) model_version: String,
    pub(super) profile_key: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncModelDto {
    pub(super) model_version_id: i64,
    pub(super) profile_key: String,
    pub(super) model_name: String,
    pub(super) version: String,
    pub(super) read_blocks: Vec<SyncReadBlockDto>,
    pub(super) fields: Vec<SyncFieldDto>,
    pub(super) bindings: Vec<SyncBindingDto>,
    #[serde(default)]
    pub(super) commands: Vec<SyncCommandDto>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncReadBlockDto {
    pub(super) id: i64,
    pub(super) function_code: u8,
    pub(super) start_address: u16,
    pub(super) register_count: u16,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncFieldDto {
    pub(super) id: i64,
    pub(super) read_block_id: i64,
    pub(super) field_name: String,
    pub(super) register_offset: u16,
    pub(super) register_length: u16,
    pub(super) value_type: String,
    #[serde(default = "default_byte_order")]
    pub(super) byte_order: String,
    pub(super) bit_offset: Option<u8>,
    pub(super) bit_length: Option<u8>,
    pub(super) decode_factor: f64,
    pub(super) decode_offset: f64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncBindingDto {
    pub(super) point_code: String,
    pub(super) protocol_field_id: i64,
    pub(super) canonical_factor: f64,
    pub(super) canonical_offset: f64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncCommandDto {
    pub(super) command_code: String,
    pub(super) command_name: String,
    pub(super) function_code: u8,
    pub(super) register_address: u16,
    pub(super) encode_type: String,
    pub(super) fixed_value: Option<u16>,
    pub(super) parameter_json: Option<serde_json::Value>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncResult {
    pub(super) applied: bool,
    pub(super) revision: String,
    pub(super) channels: usize,
    pub(super) devices: usize,
    pub(super) models: usize,
    pub(super) cloud_acknowledged: bool,
}
