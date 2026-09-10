//! 边缘网关网络管理端口。
//! Linux/RK3566 使用 NetworkManager 的 `nmcli` 前端。

use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub wifi_enabled: bool,
    pub interface: String,
    pub connected_ssid: String,
    pub signal: u8,
    pub ipv4: String,
    pub gateway: String,
    pub dns: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("NetworkManager 命令执行失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("NetworkManager 操作失败: {0}")]
    Command(String),
    #[error("未发现 Wi-Fi 网卡")]
    NoWifiInterface,
    #[error("SSID 不能为空")]
    EmptySsid,
}

pub trait NetworkManager: Send + Sync {
    fn snapshot(&self) -> Result<NetworkSnapshot, NetworkError>;
    fn cached_networks(&self) -> Result<Vec<WifiNetwork>, NetworkError>;
    fn scan(&self) -> Result<Vec<WifiNetwork>, NetworkError>;
    fn connect(&self, ssid: &str, password: &str) -> Result<(), NetworkError>;
    fn disconnect(&self) -> Result<(), NetworkError>;
    fn forget(&self, ssid: &str) -> Result<(), NetworkError>;
}

#[derive(Debug, Default)]
pub struct NmcliNetworkManager;

impl NmcliNetworkManager {
    fn output(args: &[&str]) -> Result<String, NetworkError> {
        let output = Command::new("nmcli").args(args).output()?;
        if !output.status.success() {
            return Err(NetworkError::Command(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn wifi_interface(&self) -> Result<String, NetworkError> {
        let text = Self::output(&["-t", "-f", "DEVICE,TYPE", "device", "status"])?;
        text.lines()
            .filter_map(|line| {
                split_nmcli(line)
                    .into_iter()
                    .take(2)
                    .collect::<Vec<_>>()
                    .try_into()
                    .ok()
            })
            .find_map(|[device, kind]: [String; 2]| (kind == "wifi").then_some(device))
            .ok_or(NetworkError::NoWifiInterface)
    }

    fn wifi_list(&self, rescan: bool) -> Result<Vec<WifiNetwork>, NetworkError> {
        let interface = self.wifi_interface()?;
        let text = Self::output(&[
            "-t",
            "--escape",
            "yes",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            if rescan { "yes" } else { "no" },
            "ifname",
            &interface,
        ])?;
        let mut networks = text
            .lines()
            .filter_map(|line| {
                let fields = split_nmcli(line);
                if fields.len() < 4 || fields[1].trim().is_empty() {
                    return None;
                }
                Some(WifiNetwork {
                    connected: fields[0] == "*",
                    ssid: fields[1].clone(),
                    signal: fields[2].parse::<u8>().unwrap_or_default().min(100),
                    security: if fields[3].is_empty() {
                        "开放".into()
                    } else {
                        fields[3].clone()
                    },
                })
            })
            .collect::<Vec<_>>();
        networks.sort_by_key(|item| {
            (
                std::cmp::Reverse(item.connected),
                std::cmp::Reverse(item.signal),
            )
        });
        networks.dedup_by(|left, right| left.ssid == right.ssid);
        Ok(networks)
    }
}

impl NetworkManager for NmcliNetworkManager {
    fn snapshot(&self) -> Result<NetworkSnapshot, NetworkError> {
        let interface = self.wifi_interface()?;
        let enabled = Self::output(&["radio", "wifi"])?;
        // 状态快照只读 NetworkManager 缓存；强制扫描只在用户点击“扫描”时执行。
        let active = self
            .wifi_list(false)?
            .into_iter()
            .find(|network| network.connected);
        let details = Self::output(&[
            "-t",
            "-f",
            "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
            "device",
            "show",
            &interface,
        ])?;
        let mut snapshot = NetworkSnapshot {
            wifi_enabled: enabled.trim() == "enabled",
            interface,
            connected_ssid: active
                .as_ref()
                .map(|item| item.ssid.clone())
                .unwrap_or_default(),
            signal: active.as_ref().map(|item| item.signal).unwrap_or_default(),
            ..NetworkSnapshot::default()
        };
        for line in details.lines() {
            let fields = split_nmcli(line);
            if fields.len() < 2 {
                continue;
            }
            let value = &fields[1];
            if fields[0].starts_with("IP4.ADDRESS") && snapshot.ipv4.is_empty() {
                snapshot.ipv4.clone_from(value);
            }
            if fields[0] == "IP4.GATEWAY" {
                snapshot.gateway.clone_from(value);
            }
            if fields[0].starts_with("IP4.DNS") && snapshot.dns.is_empty() {
                snapshot.dns.clone_from(value);
            }
        }
        Ok(snapshot)
    }

    fn cached_networks(&self) -> Result<Vec<WifiNetwork>, NetworkError> {
        self.wifi_list(false)
    }

    fn scan(&self) -> Result<Vec<WifiNetwork>, NetworkError> {
        self.wifi_list(true)
    }

    fn connect(&self, ssid: &str, password: &str) -> Result<(), NetworkError> {
        let ssid = ssid.trim();
        if ssid.is_empty() {
            return Err(NetworkError::EmptySsid);
        }
        let interface = self.wifi_interface()?;
        let mut args = vec![
            "--wait".to_string(),
            "25".to_string(),
            "device".to_string(),
            "wifi".to_string(),
            "connect".to_string(),
            ssid.to_string(),
        ];
        if !password.is_empty() {
            args.push("password".to_string());
            args.push(password.to_string());
        }
        args.push("ifname".to_string());
        args.push(interface);
        let output = Command::new("nmcli").args(&args).output()?;
        if !output.status.success() {
            return Err(NetworkError::Command(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }

    fn disconnect(&self) -> Result<(), NetworkError> {
        let interface = self.wifi_interface()?;
        Self::output(&["device", "disconnect", &interface]).map(|_| ())
    }

    fn forget(&self, ssid: &str) -> Result<(), NetworkError> {
        if ssid.trim().is_empty() {
            return Err(NetworkError::EmptySsid);
        }
        Self::output(&["connection", "delete", "id", ssid.trim()]).map(|_| ())
    }
}

fn split_nmcli(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}
