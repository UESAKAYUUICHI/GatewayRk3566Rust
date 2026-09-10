//! UI → agent 的用户命令（mpsc 通道，UI 回调里只 try_send）。

use gw_store::MeterInput;

#[derive(Debug, Clone)]
pub enum UserCommand {
    /// 保存云端对接参数（broker 连接参数重启进程后生效；网关 ID/SN 立即生效于后续报文）
    SaveLink {
        gateway_id: String,
        gateway_sn: String,
        mqtt_host: String,
        mqtt_port: u16,
        mqtt_username: String,
        mqtt_password: String,
    },
    SetHeartbeat {
        seconds: u64,
    },
    /// 新增（id=None）或更新电表档案，随后重启该表采集任务
    SaveMeter {
        id: Option<i64>,
        input: MeterInput,
    },
    DeleteMeter {
        id: i64,
    },
    /// 平台配置已原子写入 SQLite，重载档案并按差异重启采集任务。
    ReloadConfiguration,
    /// 手动立即读取一次并出网
    ReadNow {
        meter_id: i64,
    },
    SetContinuousPull {
        meter_id: i64,
        enabled: bool,
    },
    /// 云链路诊断（结果经事件通道回显）
    DiagnoseCloud,
}
