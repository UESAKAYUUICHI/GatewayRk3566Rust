use super::*;
pub(super) fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}天 {hours:02}:{minutes:02}")
    } else {
        format!("{hours:02}:{minutes:02}")
    }
}
pub(super) fn item(
    key: impl Into<String>,
    value: impl Into<String>,
) -> SystemInfoItemDto {
    SystemInfoItemDto {
        key: key.into(),
        value: value.into(),
    }
}
pub(super) fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
pub(super) fn read_os_release() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}
pub(super) async fn read_cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    for key in ["Hardware", "model name", "Processor"] {
        if let Some(line) = text.lines().find(|line| line.starts_with(key)) {
            return line.split_once(':').map(|(_, value)| value.trim().to_string());
        }
    }
    if let Some(model) = read_trim("/proc/device-tree/model") {
        return Some(model.trim_end_matches('\0').to_string());
    }
    let lscpu = command_output("lscpu", &[]).await;
    lscpu
        .lines()
        .find_map(|line| {
            line.strip_prefix("Model name:")
                .or_else(|| line.strip_prefix("Architecture:"))
                .map(|value| value.trim().to_string())
        })
}
pub(super) fn read_memory_info() -> Vec<SystemInfoItemDto> {
    let mut values = BTreeMap::new();
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    ["MemTotal", "MemAvailable", "MemFree", "Buffers", "Cached", "SwapTotal", "SwapFree"]
        .into_iter()
        .map(|key| item(key, values.get(key).cloned().unwrap_or_else(|| "--".into())))
        .collect()
}
pub(super) fn read_thermal_zones() -> Vec<SystemInfoItemDto> {
    let mut zones = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let label = fs::read_to_string(path.join("type"))
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| name.to_string());
            let value = fs::read_to_string(path.join("temp"))
                .ok()
                .and_then(|text| text.trim().parse::<f64>().ok())
                .map(|raw| {
                    let celsius = if raw > 1000.0 { raw / 1000.0 } else { raw };
                    format!("{celsius:.1} °C")
                })
                .unwrap_or_else(|| "--".into());
            zones.push(item(label, value));
        }
    }
    zones.sort_by(|a, b| a.key.cmp(&b.key));
    zones
}
pub(super) async fn read_storage_info() -> Vec<SystemInfoItemDto> {
    let output = command_output("df", &["-h", "/", "/usr/share/park-gateway", "/tmp"])
        .await;
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 6 {
                return None;
            }
            Some(
                item(
                    parts[5],
                    format!(
                        "{} 已用 {} / 可用 {} / 使用率 {}", parts[1], parts[2],
                        parts[3], parts[4]
                    ),
                ),
            )
        })
        .collect()
}
pub(super) async fn read_kernel_params() -> Vec<SystemInfoItemDto> {
    let keys = [
        "kernel.ostype",
        "kernel.osrelease",
        "kernel.version",
        "kernel.hostname",
        "vm.swappiness",
        "vm.dirty_ratio",
        "vm.dirty_background_ratio",
        "fs.file-max",
        "fs.inotify.max_user_watches",
        "net.ipv4.ip_forward",
        "net.core.somaxconn",
        "net.ipv4.tcp_keepalive_time",
    ];
    let mut rows = Vec::new();
    for key in keys {
        rows.push(item(key, command_output("sysctl", &["-n", key]).await));
    }
    rows
}
pub(super) async fn command_output(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() { "--".into() } else { text }
        }
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if text.is_empty() { "--".into() } else { text }
        }
        Err(_) => "--".into(),
    }
}
