use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterDto {
    pub(super) id: i64,
    pub(super) sn: String,
    pub(super) name: String,
    pub(super) profile: String,
    pub(super) model_version: String,
    pub(super) channel_id: String,
    pub(super) config_source: String,
    pub(super) upload_enabled: bool,
    pub(super) enabled: bool,
    pub(super) collect_interval_s: u64,
    pub(super) continuous_pull: bool,
    pub(super) address: u8,
    pub(super) online: bool,
    pub(super) voltage: f64,
    pub(super) current: f64,
    pub(super) power: f64,
    pub(super) energy: f64,
    pub(super) last_read: String,
    pub(super) points: Vec<PointDto>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointDto {
    pub(super) code: String,
    pub(super) name: String,
    pub(super) value: Option<f64>,
    pub(super) unit: String,
    pub(super) quality: u32,
    pub(super) collect_time: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageQuery {
    pub(super) page_num: Option<u32>,
    pub(super) page_size: Option<u32>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterRequest {
    pub(super) device_sn: String,
    #[serde(default)]
    pub(super) device_name: String,
    pub(super) modbus_addr: u8,
    pub(super) profile: String,
    #[serde(default = "default_channel_id")]
    pub(super) channel_id: String,
    #[serde(default)]
    pub(super) upload_enabled: bool,
    pub(super) collect_interval_s: u64,
    pub(super) enabled: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindMeterRequest {
    pub(super) channel_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContinuousPullRequest {
    pub(super) enabled: bool,
}
