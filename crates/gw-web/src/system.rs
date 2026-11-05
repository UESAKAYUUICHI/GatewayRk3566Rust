use super::*;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SystemEnvironmentDto {
    pub(super) hostname: String,
    pub(super) os_release: String,
    pub(super) kernel: String,
    pub(super) architecture: String,
    pub(super) cpu_model: String,
    pub(super) cpu_cores: usize,
    pub(super) cpu_temperature: String,
    pub(super) load_average: String,
    pub(super) memory: Vec<SystemInfoItemDto>,
    pub(super) storage: Vec<SystemInfoItemDto>,
    pub(super) runtime: Vec<SystemInfoItemDto>,
    pub(super) kernel_params: Vec<SystemInfoItemDto>,
    pub(super) boot_params: String,
    pub(super) thermal_zones: Vec<SystemInfoItemDto>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SystemInfoItemDto {
    pub(super) key: String,
    pub(super) value: String,
}
pub(super) async fn system_environment() -> ApiResult<SystemEnvironmentDto> {
    Ok(Json(collect_system_environment().await))
}
pub(super) async fn collect_system_environment() -> SystemEnvironmentDto {
    let hostname = read_trim("/proc/sys/kernel/hostname").unwrap_or_else(|| "--".into());
    let os_release = read_os_release().unwrap_or_else(|| "--".into());
    let kernel = command_output("uname", &["-r"]).await;
    let architecture = command_output("uname", &["-m"]).await;
    let cpu_model = read_cpu_model().await.unwrap_or_else(|| "--".into());
    let cpu_cores = fs::read_to_string("/proc/cpuinfo")
        .map(|text| {
            text.lines().filter(|line| line.starts_with("processor")).count()
        })
        .unwrap_or(0);
    let thermal_zones = read_thermal_zones();
    let cpu_temperature = thermal_zones
        .iter()
        .find(|item| item.key.to_lowercase().contains("cpu"))
        .or_else(|| thermal_zones.first())
        .map(|item| item.value.clone())
        .unwrap_or_else(|| "--".into());
    let load_average = read_trim("/proc/loadavg").unwrap_or_else(|| "--".into());
    let memory = read_memory_info();
    let storage = read_storage_info().await;
    let runtime = vec![
        item("当前用户", & command_output("whoami", & []). await),
        item("系统时间", & command_output("date", & ["+%F %T %Z"]). await),
        item("启动时间", & command_output("uptime", & ["-p"]). await),
        item("进程数量", & command_output("sh", & ["-c",
        "ls /proc | grep -E '^[0-9]+$' | wc -l"]). await,),
    ];
    let kernel_params = read_kernel_params().await;
    let boot_params = read_trim("/proc/cmdline").unwrap_or_else(|| "--".into());
    SystemEnvironmentDto {
        hostname,
        os_release,
        kernel,
        architecture,
        cpu_model,
        cpu_cores,
        cpu_temperature,
        load_average,
        memory,
        storage,
        runtime,
        kernel_params,
        boot_params,
        thermal_zones,
    }
}
pub(super) async fn system_diagnostics(
    State(state): State<WebState>,
) -> Result<Response, ApiError> {
    let environment = collect_system_environment().await;
    let snapshot = build_snapshot(&state).await?;
    let report = format!(
        "# Park Gateway Diagnostics\n\n## Snapshot\n{}\n\n## Environment\n{}\n",
        serde_json::to_string_pretty(& snapshot).unwrap_or_else(| _ | "{}".into()),
        serde_json::to_string_pretty(& environment).unwrap_or_else(| _ | "{}".into()),
    );
    Ok(
        (
            [
                ("content-type", "text/plain; charset=utf-8"),
                (
                    "content-disposition",
                    "attachment; filename=\"park-gateway-diagnostics.txt\"",
                ),
            ],
            report,
        )
            .into_response(),
    )
}
pub(super) async fn restart_service() -> ApiResult<Accepted> {
    let pid = std::process::id();
    let script = format!(
        "(sleep 1; systemctl restart park-gateway.service || kill -TERM {pid}) >/tmp/park-gateway-service-restart.log 2>&1 &"
    );
    Command::new("sh")
        .args(["-c", &script])
        .spawn()
        .map_err(|error| ApiError::internal(
            format!("无法发起服务重启: {error}"),
        ))?;
    Ok(Json(Accepted { accepted: true }))
}
pub(super) async fn shutdown_machine() -> ApiResult<Accepted> {
    schedule_power_action(
        "systemctl poweroff",
        "/tmp/park-gateway-poweroff.log",
        "关机",
    )
}
pub(super) async fn reboot_machine() -> ApiResult<Accepted> {
    schedule_power_action(
        "systemctl reboot",
        "/tmp/park-gateway-reboot.log",
        "重启机器",
    )
}
pub(super) fn schedule_power_action(
    command: &str,
    log_path: &str,
    label: &str,
) -> ApiResult<Accepted> {
    let script = format!("(sleep 1; {command}) >{log_path} 2>&1 &");
    Command::new("sh")
        .args(["-c", &script])
        .spawn()
        .map_err(|error| ApiError::internal(format!("无法发起{label}: {error}")))?;
    Ok(Json(Accepted { accepted: true }))
}
