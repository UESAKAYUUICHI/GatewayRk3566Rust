//! 指令语义解释。云侧 `PlatformBusinessQueryService.commandTypes` 定义了三类指令：
//! READ_NOW（立即读取，目标 DEVICE）/ SET_INTERVAL（设置上报间隔，目标 GATEWAY）/
//! REBOOT_GATEWAY（重启网关，目标 GATEWAY）。

use gw_proto::CommandBody;

/// 云侧指令解释为网关本地动作。
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayAction {
    /// 立即读取目标设备并单发一帧数据报文
    ReadNow { target_sn: String },
    /// 修改默认采集周期（秒），持久化到运行时配置
    SetInterval { seconds: u64 },
    /// 重启网关（回执后延迟执行）
    Reboot,
    /// 未知指令类型：回执 FAILED
    Unknown { kind: String },
}

impl GatewayAction {
    /// 动作名称（日志/UI 用）。
    pub fn name(&self) -> &'static str {
        match self {
            GatewayAction::ReadNow { .. } => "READ_NOW",
            GatewayAction::SetInterval { .. } => "SET_INTERVAL",
            GatewayAction::Reboot => "REBOOT_GATEWAY",
            GatewayAction::Unknown { .. } => "UNKNOWN",
        }
    }
}

/// 解释云侧指令体。payload 兼容 `{"seconds":300}` 与 `{"intervalSeconds":300}` 两种键。
pub fn interpret(body: &CommandBody) -> GatewayAction {
    match body.command_type.as_deref() {
        Some("READ_NOW") => GatewayAction::ReadNow {
            target_sn: body.target_sn.clone().unwrap_or_default(),
        },
        Some("SET_INTERVAL") => {
            let seconds = body
                .payload
                .get("seconds")
                .or_else(|| body.payload.get("intervalSeconds"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            GatewayAction::SetInterval { seconds }
        }
        Some("REBOOT_GATEWAY") => GatewayAction::Reboot,
        other => GatewayAction::Unknown {
            kind: other.unwrap_or("<missing>").to_string(),
        },
    }
}
