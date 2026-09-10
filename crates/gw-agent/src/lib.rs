//! 网关 agent：任务编排层。UI 无关 —— 同一套 runner 服务于 Slint 前端与 headless 二进制。

pub mod config;
pub mod runner;
pub mod state;
pub mod transport_registry;
pub mod ui_command;

pub use config::{BootstrapConfig, CloudSection, MeterSeed, SerialSection};
pub use runner::{
    AgentDeps, AgentHandle, ClockSource, RealClock, RebootHook, StdReboot, spawn_agent,
};
pub use state::MeterRuntime;
pub use transport_registry::{TransportHandle, TransportRegistry};
pub use ui_command::UserCommand;
