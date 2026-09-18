//! park-gateway 主二进制：Slint 触屏 UI + 完整网关管线。
//!
//! 线程模型（设计文档 §7.2）：
//! - 主线程：Slint 事件循环（部分平台强制 UI 在主线程）
//! - tokio 运行时：agent 全部任务（采集/发布/心跳/指令/快照）
//! - UI→核心：mpsc（回调里只 try_send）
//! - 核心→UI：watch 快照 1Hz 合拍，经 invoke_from_event_loop 更新属性/模型

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gw_agent::runner::{NoopReboot, StdReboot};
use gw_agent::{
    AgentDeps, BootstrapConfig, RealClock, TransportRegistry, UserCommand, spawn_agent,
};
use gw_collector::MeterTransport;
use gw_core::snapshot::Snapshot;
use gw_link::port::{CloudLink, LinkOptions};
use gw_network::{NetworkManager, NetworkSnapshot, NmcliNetworkManager, WifiNetwork};
use gw_store::Store;
use slint::{ComponentHandle, Model, VecModel};
use tokio::sync::{Mutex, mpsc};

slint::include_modules!();

fn parse_args() -> Option<PathBuf> {
    let mut config = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = args.next().map(PathBuf::from),
            other => {
                eprintln!("未知参数: {other}");
                std::process::exit(2);
            }
        }
    }
    config
}

fn load_bootstrap(config_path: &Option<PathBuf>) -> anyhow::Result<BootstrapConfig> {
    match config_path {
        Some(path) => BootstrapConfig::load(path),
        None => {
            // 查找顺序：/etc/park-gateway/gateway.toml > ./config/gateway.toml > 内置默认
            let candidates = [
                PathBuf::from("/etc/park-gateway/gateway.toml"),
                PathBuf::from("config/gateway.toml"),
            ];
            match candidates.into_iter().find(|p| p.exists()) {
                Some(path) => BootstrapConfig::load(&path),
                None => Ok(BootstrapConfig::default()),
            }
        }
    }
}

/// HH:MM:SS（本地时区，偏移固定在配置中）。
fn fmt_time(ms: u64, tz_offset_s: i32) -> String {
    if ms == 0 {
        return "-".to_string();
    }
    let shifted = ms as i64 + tz_offset_s as i64 * 1000;
    let day_ms = shifted.rem_euclid(86_400_000);
    let h = day_ms / 3_600_000;
    let m = (day_ms % 3_600_000) / 60_000;
    let s = (day_ms % 60_000) / 1000;
    format!("{h:02}:{m:02}:{s:02}")
}

fn fmt_uptime(seconds: u64) -> String {
    let d = seconds / 86_400;
    let h = (seconds % 86_400) / 3_600;
    let m = (seconds % 3_600) / 60;
    let s = seconds % 60;
    if d > 0 {
        format!("{d}天 {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_network_state(
    manager: &dyn NetworkManager,
) -> Result<(NetworkSnapshot, Vec<WifiNetwork>), String> {
    let snapshot = manager.snapshot().map_err(|error| error.to_string())?;
    let networks = manager.scan().map_err(|error| error.to_string())?;
    Ok((snapshot, networks))
}

fn apply_network_state(
    window: &GatewayWindow,
    snapshot: NetworkSnapshot,
    networks: Vec<WifiNetwork>,
    message: &str,
) {
    let connected = !snapshot.connected_ssid.is_empty();
    window.set_wifi_ok(connected);
    window.set_wifi_status(
        if !snapshot.wifi_enabled {
            "Wi-Fi 已关闭"
        } else if connected {
            "已连接"
        } else {
            "Wi-Fi 可用，尚未连接"
        }
        .into(),
    );
    window.set_wifi_ssid(snapshot.connected_ssid.clone().into());
    window.set_wifi_signal(snapshot.signal as i32);
    window.set_wifi_ipv4(snapshot.ipv4.into());
    window.set_wifi_gateway(snapshot.gateway.into());
    window.set_wifi_dns(snapshot.dns.into());
    window.set_wifi_interface(snapshot.interface.into());
    if connected && window.get_wifi_selected_ssid().is_empty() {
        window.set_wifi_selected_ssid(snapshot.connected_ssid.into());
    }
    let rows = networks
        .into_iter()
        .map(|network| WifiRow {
            ssid: network.ssid.into(),
            signal: network.signal as i32,
            security: network.security.into(),
            connected: network.connected,
        })
        .collect::<Vec<_>>();
    window.set_wifi_networks(Rc::new(VecModel::from(rows)).into());
    window.set_wifi_message(message.into());
}

/// 页面内不堆叠错误提示；需要人工处理的结果统一通过 Slint 弹窗呈现。
fn show_dialog(
    window: &GatewayWindow,
    title: &str,
    message: impl Into<slint::SharedString>,
    danger: bool,
) {
    window.set_dialog_title(title.into());
    window.set_dialog_message(message.into());
    window.set_dialog_danger(danger);
    window.set_dialog_open(true);
}

fn refresh_network_async(
    manager: Arc<dyn NetworkManager>,
    weak: slint::Weak<GatewayWindow>,
    success_message: &'static str,
) {
    std::thread::Builder::new()
        .name("wifi-refresh".into())
        .spawn(move || {
            let result = load_network_state(manager.as_ref());
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok((snapshot, networks)) => {
                        apply_network_state(&window, snapshot, networks, success_message)
                    }
                    Err(error) => {
                        window.set_wifi_ok(false);
                        window.set_wifi_status("网络服务不可用".into());
                        window.set_wifi_message(error.clone().into());
                        show_dialog(&window, "网络服务不可用", error, true);
                    }
                }
            });
        })
        .expect("spawn wifi-refresh");
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gw_agent=debug".into()),
        )
        .init();

    let config_path = parse_args();
    let bootstrap = load_bootstrap(&config_path)?;
    let tz_offset_s = bootstrap.tz_offset_s;
    let bootstrap_gateway_id = bootstrap.cloud.gateway_id.clone();
    let bootstrap_gateway_sn = bootstrap.cloud.gateway_sn.clone();
    let bootstrap_mqtt_host = bootstrap.cloud.mqtt_host.clone();
    let bootstrap_mqtt_port = bootstrap.cloud.mqtt_port.to_string();

    // tokio 运行时（主线程短暂 block_on 完成装配后保持存活，worker 线程跑 agent 任务）
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = runtime.block_on(async {
        let store = Arc::new(
            Store::open(std::path::Path::new(&bootstrap.db_path)).or_else(|_| {
                // Linux 数据目录不可写（开发机）时退到本地
                tracing::warn!("数据目录不可用，退到本地 gateway.db");
                Store::open(std::path::Path::new("gateway.db"))
            })?,
        );

        // 云链路
        let (cmd_tx_for_link, command_rx) = mpsc::channel(16);
        let stored = |key: &str| store.get_config(key).ok().flatten();
        let opts = LinkOptions {
            host: stored("mqtt_host").unwrap_or_else(|| bootstrap.cloud.mqtt_host.clone()),
            port: stored("mqtt_port")
                .and_then(|v| v.parse().ok())
                .unwrap_or(bootstrap.cloud.mqtt_port),
            username: stored("mqtt_username")
                .unwrap_or_else(|| bootstrap.cloud.mqtt_username.clone()),
            password: stored("mqtt_password")
                .unwrap_or_else(|| bootstrap.cloud.mqtt_password.clone()),
            client_id: format!("park-gateway-{}", bootstrap.cloud.gateway_id),
            gateway_id: bootstrap.cloud.gateway_id.clone(),
        };
        let topic = gw_proto::topics::command_down_topic(&opts.gateway_id);
        let (mqtt, link_task) = gw_link::MqttLink::spawn(opts, topic, cmd_tx_for_link);
        tokio::spawn(async move {
            let _ = link_task.await;
        });
        let link: Arc<dyn CloudLink> = Arc::new(mqtt);

        // 采集传输
        let serial = gw_collector::SerialConfig {
            port: bootstrap.serial.port.clone(),
            baud: bootstrap.serial.baud,
            data_bits: bootstrap.serial.data_bits,
            stop_bits: bootstrap.serial.stop_bits,
            parity: bootstrap.serial.parity.clone(),
            timeout_ms: 1_000,
            retry_count: 2,
        };
        let connected = gw_collector::ModbusRtuTransport::connect(&serial)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let transport: Arc<Mutex<dyn MeterTransport>> = Arc::new(Mutex::new(connected));
        let transports = Arc::new(TransportRegistry::default());
        transports.insert("rs485-1", transport);

        let shutdown = tokio_util::sync::CancellationToken::new();
        Ok::<_, anyhow::Error>(spawn_agent(
            AgentDeps {
                store: store.clone(),
                link,
                transports,
                clock: Arc::new(RealClock),
                reboot: if cfg!(target_os = "linux") {
                    Arc::new(StdReboot)
                } else {
                    // 非 Linux（开发机演示）不真重启
                    Arc::new(NoopReboot)
                },
                bootstrap,
            },
            command_rx,
            shutdown.clone(),
        ))
    })?;

    // ---- UI 装配（主线程）----
    let window = GatewayWindow::new()?;
    let store = handle.store.clone();
    let cmd_tx = handle.cmd_tx.clone();
    let network: Arc<dyn NetworkManager> = Arc::new(NmcliNetworkManager);

    // 表单回填：运行时配置 > 引导默认
    let fallbacks = [
        ("gateway_id", bootstrap_gateway_id.as_str()),
        ("gateway_sn", bootstrap_gateway_sn.as_str()),
        ("mqtt_host", bootstrap_mqtt_host.as_str()),
        ("mqtt_port", bootstrap_mqtt_port.as_str()),
        ("mqtt_username", ""),
        ("mqtt_password", ""),
    ];
    for (key, fallback) in fallbacks {
        let value = store
            .get_config(key)
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| fallback.to_string());
        match key {
            "gateway_id" => window.set_cfg_gateway_id(value.into()),
            "gateway_sn" => window.set_cfg_gateway_sn(value.into()),
            "mqtt_host" => window.set_cfg_mqtt_host(value.into()),
            "mqtt_port" => window.set_cfg_mqtt_port(value.into()),
            "mqtt_username" => window.set_cfg_mqtt_user(value.into()),
            _ => window.set_cfg_mqtt_pass(value.into()),
        }
    }

    window.set_version(env!("CARGO_PKG_VERSION").into());

    // ---- 回调：全部只 try_send / 短临界区读库 ----
    window.on_nav_clicked({
        let weak = window.as_weak();
        let store = store.clone();
        move |index| {
            if let Some(w) = weak.upgrade() {
                w.set_page(index);
                if index == 2 {
                    w.invoke_wifi_scan();
                }
                if index == 3 || index == 4 {
                    refresh_logs(&w, &store, tz_offset_s);
                }
            }
        }
    });
    window.on_save_link({
        let cmd_tx = cmd_tx.clone();
        let weak = window.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let _ = cmd_tx.try_send(UserCommand::SaveLink {
                    gateway_id: w.get_cfg_gateway_id().trim().to_string(),
                    gateway_sn: w.get_cfg_gateway_sn().trim().to_string(),
                    mqtt_host: w.get_cfg_mqtt_host().trim().to_string(),
                    mqtt_port: w.get_cfg_mqtt_port().trim().parse().unwrap_or(1883),
                    mqtt_username: w.get_cfg_mqtt_user().trim().to_string(),
                    mqtt_password: w.get_cfg_mqtt_pass().to_string(),
                });
            }
        }
    });
    window.on_diagnose_cloud({
        let cmd_tx = cmd_tx.clone();
        move || {
            let _ = cmd_tx.try_send(UserCommand::DiagnoseCloud);
        }
    });
    window.on_save_meter({
        let cmd_tx = cmd_tx.clone();
        let weak = window.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let _ = cmd_tx.try_send(UserCommand::SaveMeter {
                    id: (w.get_edit_id() != 0).then_some(w.get_edit_id() as i64),
                    input: gw_store::MeterInput {
                        device_sn: w.get_edit_sn().trim().to_string(),
                        device_name: w.get_edit_sn().trim().to_string(),
                        modbus_addr: w.get_edit_addr().trim().parse().unwrap_or(1),
                        profile: {
                            let profile = w.get_edit_profile().trim().to_string();
                            if profile.is_empty() {
                                "PD666-3S3".to_string()
                            } else {
                                profile
                            }
                        },
                        channel_id: "rs485-1".to_string(),
                        upload_enabled: false,
                        collect_interval_s: w.get_edit_interval().trim().parse().unwrap_or(300),
                        enabled: true,
                    },
                });
            }
        }
    });
    window.on_new_meter({
        let weak = window.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                w.set_edit_id(0);
                w.set_edit_sn("".into());
                w.set_edit_addr("1".into());
                w.set_edit_interval("300".into());
                w.set_edit_profile("PD666-3S3".into());
                w.set_page(1);
            }
        }
    });
    window.on_delete_meter({
        let cmd_tx = cmd_tx.clone();
        move |id| {
            let _ = cmd_tx.try_send(UserCommand::DeleteMeter { id: id as i64 });
        }
    });
    window.on_read_now({
        let cmd_tx = cmd_tx.clone();
        move |id| {
            let _ = cmd_tx.try_send(UserCommand::ReadNow {
                meter_id: id as i64,
            });
        }
    });

    // ---- 网络管理：Linux 使用 NetworkManager；耗时操作全部离开 UI 线程 ----
    window.on_wifi_scan({
        let weak = window.as_weak();
        let network = network.clone();
        move || {
            if let Some(w) = weak.upgrade() {
                w.set_wifi_message("正在扫描附近网络…".into());
            }
            refresh_network_async(network.clone(), weak.clone(), "扫描完成");
        }
    });
    window.on_wifi_select({
        let weak = window.as_weak();
        move |index| {
            let Some(w) = weak.upgrade() else { return };
            if let Some(network) = w.get_wifi_networks().row_data(index as usize) {
                w.set_wifi_selected_ssid(network.ssid);
                w.set_wifi_password("".into());
                w.set_wifi_message(
                    if network.connected {
                        "当前网络已连接"
                    } else {
                        "请输入密码后连接"
                    }
                    .into(),
                );
            }
        }
    });
    window.on_wifi_connect({
        let weak = window.as_weak();
        let network = network.clone();
        move || {
            let Some(w) = weak.upgrade() else { return };
            let ssid = w.get_wifi_selected_ssid().trim().to_string();
            let password = w.get_wifi_password().to_string();
            if ssid.is_empty() {
                w.set_wifi_message("请先选择或输入 SSID".into());
                return;
            }
            w.set_wifi_password("".into());
            w.set_wifi_message(format!("正在连接 {ssid}…").into());
            let manager = network.clone();
            let weak = weak.clone();
            std::thread::Builder::new()
                .name("wifi-connect".into())
                .spawn(move || {
                    let action = manager.connect(&ssid, &password).map_err(|e| e.to_string());
                    let state = action.and_then(|_| load_network_state(manager.as_ref()));
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else { return };
                        match state {
                            Ok((snapshot, networks)) => apply_network_state(
                                &window,
                                snapshot,
                                networks,
                                &format!("已连接 {ssid}"),
                            ),
                            Err(error) => {
                                let message = format!("连接失败：{error}");
                                window.set_wifi_message(message.clone().into());
                                show_dialog(&window, "Wi-Fi 连接失败", message, true);
                            }
                        }
                    });
                })
                .expect("spawn wifi-connect");
        }
    });
    window.on_wifi_disconnect({
        let weak = window.as_weak();
        let network = network.clone();
        move || {
            if let Some(w) = weak.upgrade() {
                w.set_wifi_message("正在断开 Wi-Fi…".into());
            }
            let manager = network.clone();
            let weak = weak.clone();
            std::thread::Builder::new()
                .name("wifi-disconnect".into())
                .spawn(move || {
                    let state = manager
                        .disconnect()
                        .map_err(|e| e.to_string())
                        .and_then(|_| load_network_state(manager.as_ref()));
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else { return };
                        match state {
                            Ok((snapshot, networks)) => {
                                apply_network_state(&window, snapshot, networks, "Wi-Fi 已断开")
                            }
                            Err(error) => {
                                let message = format!("断开失败：{error}");
                                window.set_wifi_message(message.clone().into());
                                show_dialog(&window, "Wi-Fi 断开失败", message, true);
                            }
                        }
                    });
                })
                .expect("spawn wifi-disconnect");
        }
    });
    window.on_wifi_forget({
        let weak = window.as_weak();
        let network = network.clone();
        move || {
            let Some(w) = weak.upgrade() else { return };
            let ssid = w.get_wifi_selected_ssid().trim().to_string();
            if ssid.is_empty() {
                w.set_wifi_message("请选择需要忘记的网络".into());
                return;
            }
            w.set_wifi_password("".into());
            w.set_wifi_message(format!("正在移除 {ssid}…").into());
            let manager = network.clone();
            let weak = weak.clone();
            std::thread::Builder::new()
                .name("wifi-forget".into())
                .spawn(move || {
                    let state = manager
                        .forget(&ssid)
                        .map_err(|e| e.to_string())
                        .and_then(|_| load_network_state(manager.as_ref()));
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else { return };
                        match state {
                            Ok((snapshot, networks)) => {
                                window.set_wifi_selected_ssid("".into());
                                apply_network_state(&window, snapshot, networks, "已移除网络配置");
                            }
                            Err(error) => {
                                let message = format!("移除失败：{error}");
                                window.set_wifi_message(message.clone().into());
                                show_dialog(&window, "移除 Wi-Fi 失败", message, true);
                            }
                        }
                    });
                })
                .expect("spawn wifi-forget");
        }
    });
    window.set_wifi_message("正在读取网络状态…".into());
    refresh_network_async(network.clone(), window.as_weak(), "网络状态已更新");

    // ---- 核心→UI 桥：watch 快照 → invoke_from_event_loop（1s 轮询与 1Hz 快照合拍）----
    let weak = window.as_weak();
    let mut snapshot_rx = handle.snapshot_rx.clone();
    std::thread::Builder::new()
        .name("ui-bridge".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if snapshot_rx.has_changed().unwrap_or(false) {
                    let snapshot = snapshot_rx.borrow_and_update().clone();
                    let weak = weak.clone();
                    let result = slint::invoke_from_event_loop(move || {
                        let Some(w) = weak.upgrade() else { return };
                        apply_snapshot(&w, &snapshot, tz_offset_s);
                    });
                    if result.is_err() {
                        return; // UI 已退出
                    }
                }
            }
        })
        .expect("spawn ui-bridge");

    let exit = window.run();
    handle.shutdown.cancel();
    drop(runtime); // 停 worker 任务
    exit?;
    Ok(())
}

/// 把快照写入 UI 属性与电表模型（UI 线程内调用）。
fn apply_snapshot(window: &GatewayWindow, snapshot: &Snapshot, tz: i32) {
    window.set_cloud_ok(matches!(
        snapshot.cloud,
        gw_core::snapshot::LinkHealth::Connected
    ));
    window.set_cloud_text(snapshot.cloud.label().into());
    window.set_clock_ok(snapshot.clock_trusted);
    window.set_clock_text(
        if snapshot.clock_trusted {
            "时钟已同步"
        } else {
            "时钟未同步（数据暂存不出网）"
        }
        .into(),
    );
    window.set_outbox_pending(snapshot.outbox_pending as i32);
    window.set_outbox_sent(snapshot.outbox_sent_total as i32);
    window.set_meters_online(snapshot.meters_online as i32);
    window.set_meters_total(snapshot.meters_total as i32);
    window.set_today_kwh(snapshot.today_kwh_total as f32);
    window.set_last_publish(fmt_time(snapshot.last_publish_ms, tz).into());
    window.set_uptime(fmt_uptime(snapshot.uptime_s(now_ms())).into());

    let rows: Vec<MeterRow> = snapshot
        .meters
        .iter()
        .map(|m| {
            let point = |key: &str| {
                m.points
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| *v)
                    .unwrap_or(0.0)
            };
            MeterRow {
                id: m.id as i32,
                device_sn: m.device_sn.clone().into(),
                online: m.online,
                addr: m.addr as i32,
                interval: m.interval_s as i32,
                enabled: m.enabled,
                profile: m.profile.clone().into(),
                last_read: fmt_time(m.last_read_ms, tz).into(),
                voltage_a: point("voltage_a") as f32,
                current_a: point("current_a") as f32,
                power_kw: point("active_power_total") as f32,
                energy_kwh: point("forward_active_energy") as f32,
                today_kwh: m.today_kwh as f32,
            }
        })
        .collect();
    window.set_meters(Rc::new(VecModel::from(rows)).into());
}

/// 拉取指令/事件日志到 UI（UI 线程内调用）。
fn refresh_logs(window: &GatewayWindow, store: &Store, tz: i32) {
    let commands: Vec<CmdRow> = store
        .recent_commands(50)
        .unwrap_or_default()
        .into_iter()
        .map(|c| CmdRow {
            command_id: c.command_id.into(),
            command_type: c.command_type.into(),
            status: c.result_status.into(),
            message: c.message.into(),
        })
        .collect();
    window.set_commands(Rc::new(VecModel::from(commands)).into());

    let events: Vec<EventRowLite> = store
        .recent_events(100)
        .unwrap_or_default()
        .into_iter()
        .map(|e| EventRowLite {
            time: fmt_time(e.ts_ms, tz).into(),
            level: e.level.into(),
            source: e.source.into(),
            message: e.message.into(),
        })
        .collect();
    window.set_events(Rc::new(VecModel::from(events)).into());
}
