//! headless 网关二进制：无 UI，agent + 真驱动。
//!
//! 用法示例：
//! - 真机：  gateway-headless --config /etc/park-gateway/gateway.toml

use std::path::PathBuf;
use std::sync::Arc;

use gw_agent::{AgentDeps, BootstrapConfig, RealClock, StdReboot, TransportRegistry, spawn_agent};
use gw_link::port::{CloudLink, LinkOptions};
use gw_store::Store;

fn parse_args() -> Option<PathBuf> {
    let all: Vec<String> = std::env::args().collect();
    let mut config = None;
    let mut args = all.into_iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config = args.next().map(PathBuf::from);
            }
            other => {
                eprintln!("未知参数: {other}");
                std::process::exit(2);
            }
        }
    }
    config
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gw_agent=debug".into()),
        )
        .init();

    let config_path = parse_args();
    let bootstrap = match &config_path {
        Some(path) => BootstrapConfig::load(path)?,
        None => BootstrapConfig::default(),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let store = Arc::new(Store::open(std::path::Path::new(&bootstrap.db_path))?);
        if store.get_config("rs485_initialized")?.is_none() {
            if let Some(mut channel) = store
                .channels()?
                .into_iter()
                .find(|channel| channel.id == "rs485-1")
            {
                channel.port = bootstrap.serial.port.clone();
                channel.baud = bootstrap.serial.baud;
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
            username: stored("mqtt_username")
                .unwrap_or_else(|| bootstrap.cloud.mqtt_username.clone()),
            password: stored("mqtt_password")
                .unwrap_or_else(|| bootstrap.cloud.mqtt_password.clone()),
            client_id: format!("park-gateway-{}", bootstrap.cloud.gateway_id),
            gateway_id: bootstrap.cloud.gateway_id.clone(),
        };
        let topic = gw_proto::topics::command_down_topic(&opts.gateway_id);
        let (mqtt, link_task) = gw_link::MqttLink::spawn(opts, topic, tx);
        let link = Arc::new(mqtt) as Arc<dyn CloudLink>;

        let transports = Arc::new(TransportRegistry::default());
        for channel in store.channels()?.into_iter().filter(|channel| channel.enabled) {
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
                    transports.insert(channel.id.clone(), Arc::new(tokio::sync::Mutex::new(transport)));
                    tracing::info!(channel = %channel.id, port = %channel.port, "RS485 通道已建立");
                }
                Err(error) => tracing::error!(channel = %channel.id, port = %channel.port, "RS485 通道建立失败: {error}"),
            }
        }

        let shutdown = tokio_util::sync::CancellationToken::new();
        let handle = spawn_agent(
            AgentDeps {
                store: store.clone(),
                link,
                transports,
                clock: Arc::new(RealClock),
                reboot: Arc::new(StdReboot),
                bootstrap,
            },
            command_rx,
            shutdown.clone(),
        );

        println!("gateway-headless 已启动（Ctrl-C 退出）");
        tokio::signal::ctrl_c().await?;
        shutdown.cancel();
        println!("正在退出…");
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), link_task).await;
        let _ = handle;
        Ok(())
    })
}
