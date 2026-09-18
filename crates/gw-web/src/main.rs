use std::{path::PathBuf, sync::Arc};

use gw_agent::{AgentDeps, BootstrapConfig, RealClock, StdReboot, TransportRegistry, spawn_agent};
use gw_link::port::{CloudLink, LinkOptions};
use gw_network::{NetworkManager, NmcliNetworkManager};
use gw_store::Store;

struct Args {
    config: Option<PathBuf>,
    web_root: PathBuf,
    bind: String,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut result = Args {
        config: None,
        web_root: "web-ui/dist".into(),
        bind: "0.0.0.0:8080".into(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                result.config = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--config 缺少路径"))?
                        .into(),
                )
            }
            "--web-root" => {
                result.web_root = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--web-root 缺少路径"))?
                    .into()
            }
            "--bind" => {
                result.bind = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--bind 缺少地址"))?
            }
            other => anyhow::bail!("未知参数: {other}"),
        }
    }
    Ok(result)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gw_agent=debug,tower_http=info".into()),
        )
        .init();
    let args = parse_args()?;
    let bootstrap = match args.config {
        Some(path) => BootstrapConfig::load(&path)?,
        None => BootstrapConfig::default(),
    };
    if let Some(parent) = std::path::Path::new(&bootstrap.db_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let store = Arc::new(Store::open(std::path::Path::new(&bootstrap.db_path))?);
    if store.get_config("rs485_initialized")?.is_none() {
        if let Some(mut channel) = store
            .channels()?
            .into_iter()
            .find(|channel| channel.id == "rs485-1")
        {
            channel.port = bootstrap.serial.port.clone();
            channel.baud = bootstrap.serial.baud;
                channel.data_bits = bootstrap.serial.data_bits;
                channel.stop_bits = bootstrap.serial.stop_bits;
                channel.parity = bootstrap.serial.parity.clone();
            store.upsert_channel(&channel, 0)?;
        }
        store.set_config("rs485_initialized", "1")?;
    }

    let (tx, command_rx) = tokio::sync::mpsc::channel(16);
    let stored = |key: &str| store.get_config(key).ok().flatten();
    let opts = LinkOptions {
        host: stored("mqtt_host").unwrap_or_else(|| bootstrap.cloud.mqtt_host.clone()),
        port: stored("mqtt_port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(bootstrap.cloud.mqtt_port),
        username: stored("mqtt_username").unwrap_or_else(|| bootstrap.cloud.mqtt_username.clone()),
        password: stored("mqtt_password").unwrap_or_else(|| bootstrap.cloud.mqtt_password.clone()),
        client_id: format!("park-gateway-{}", bootstrap.cloud.gateway_id),
        gateway_id: bootstrap.cloud.gateway_id.clone(),
    };
    let topic = gw_proto::topics::command_down_topic(&opts.gateway_id);
    let (mqtt, link_task) = gw_link::MqttLink::spawn(opts, topic, tx);
    let link = Arc::new(mqtt) as Arc<dyn CloudLink>;

    let transports = Arc::new(TransportRegistry::default());
    for channel in store
        .channels()?
        .into_iter()
        .filter(|channel| channel.enabled)
    {
        let serial = gw_collector::SerialConfig {
            port: channel.port.clone(),
            baud: channel.baud,
            data_bits: channel.data_bits,
            stop_bits: channel.stop_bits,
            parity: channel.parity.clone(),
            timeout_ms: channel.timeout_ms,
            retry_count: channel.retry_count,
        };
        match gw_collector::ModbusRtuTransport::connect(&serial).await {
            Ok(transport) => {
                transports.insert(
                    channel.id.clone(),
                    Arc::new(tokio::sync::Mutex::new(transport)),
                );
                tracing::info!(channel = %channel.id, port = %channel.port, "RS485 通道已建立");
            }
            Err(error) => {
                tracing::error!(channel = %channel.id, port = %channel.port, "RS485 通道建立失败: {error}")
            }
        }
    }
    let network: Arc<dyn NetworkManager> = Arc::new(NmcliNetworkManager);
    let shutdown = tokio_util::sync::CancellationToken::new();
    let handle = spawn_agent(
        AgentDeps {
            store: store.clone(),
            link,
            transports: transports.clone(),
            clock: Arc::new(RealClock),
            reboot: Arc::new(StdReboot),
            bootstrap: bootstrap.clone(),
        },
        command_rx,
        shutdown.clone(),
    );
    let state = gw_web::WebState {
        snapshot_rx: handle.snapshot_rx.clone(),
        command_tx: handle.cmd_tx.clone(),
        store,
        transports,
        network,
        network_cache: Arc::new(tokio::sync::Mutex::new(gw_web::NetworkCache::default())),
        bootstrap,
        runtime: gw_web::RuntimeDto::production(),
    };
    let alarm_sync_state = state.clone();
    let alarm_sync_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = alarm_sync_shutdown.cancelled() => return,
                _ = ticker.tick() => {
                    if let Err(error) = gw_web::sync_platform_alarms_once(&alarm_sync_state).await {
                        tracing::debug!("平台告警同步暂未完成: {error:?}");
                    }
                }
            }
        }
    });
    let rule_sync_state = state.clone();
    let rule_sync_shutdown = shutdown.clone();
    tokio::spawn(async move {
        loop {
            let interval_s = rule_sync_state
                .store
                .get_config("alarm_rule_sync_interval_s")
                .ok()
                .flatten()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(300)
                .clamp(30, 86_400);
            tokio::select! {
                _ = rule_sync_shutdown.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(interval_s)) => {
                    if let Err(error) = gw_web::sync_platform_alarm_rules_once(&rule_sync_state).await {
                        tracing::debug!("平台告警规则同步暂未完成: {error:?}");
                    }
                }
            }
        }
    });
    let app = gw_web::router(state, args.web_root);
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    tracing::info!(address = %args.bind, "智能 AI 采集边缘网关 Web 服务已启动");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), link_task).await;
    Ok(())
}
