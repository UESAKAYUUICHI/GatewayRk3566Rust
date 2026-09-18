//! 指令语义解释。云侧 `PlatformBusinessQueryService.commandTypes` 定义了三类指令：
//! READ_NOW（立即读取，目标 DEVICE）/ SET_INTERVAL（设置上报间隔，目标 GATEWAY）/
//! REBOOT_GATEWAY（重启网关，目标 GATEWAY）/
//! DEVICE_COMMAND（执行已发布协议的白名单命令，目标 DEVICE）。

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
    /// Execute a command from the synchronized protocol whitelist.
    DeviceCommand { target_sn: String, command_code: String },
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
            GatewayAction::DeviceCommand { .. } => "DEVICE_COMMAND",
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
        Some("DEVICE_COMMAND") | Some("EXECUTE_DEVICE_COMMAND") => GatewayAction::DeviceCommand {
            target_sn: body.target_sn.clone().unwrap_or_default(),
            command_code: body.payload.get("commandCode")
                .or_else(|| body.payload.get("command_code"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .trim()
                .to_ascii_uppercase(),
        },
        other => GatewayAction::Unknown {
            kind: other.unwrap_or("<missing>").to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protocol_whitelist_command() {
        let body = CommandBody {
            target_sn: Some("LIGHT-001".into()),
            command_type: Some("DEVICE_COMMAND".into()),
            payload: serde_json::json!({"commandCode": "loop_1_on"}),
            ..Default::default()
        };
        assert_eq!(
            interpret(&body),
            GatewayAction::DeviceCommand {
                target_sn: "LIGHT-001".into(),
                command_code: "LOOP_1_ON".into(),
            }
        );
    }
}
