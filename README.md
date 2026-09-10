# 园区能源网关（RK3566 · Rust + Slint）

园区综合能源管理系统的边缘网关：向下经 RS485（Modbus RTU）采集真实电表，
向上与 `park-energy-access` 严格协议兼容（MQTT QoS1，云侧零改动）。
设计文档：[docs/DESIGN.md](docs/DESIGN.md)；上机手册：[deploy/README.md](deploy/README.md)。
真实网关化调整记录：[docs/production-readiness.md](docs/production-readiness.md)。

## 快速开始（Linux 真机）

```bash
npm --prefix web-ui ci
npm --prefix web-ui run build
cargo run -p gw-web --bin gateway-web -- --config config/gateway.toml --web-root web-ui/dist
```

RK3568/Ubuntu 板端部署：

```bash
MODE=release TARGET=aarch64-unknown-linux-gnu ./deploy/build-arm64.sh
sudo ./deploy/install-on-device.sh
```

## Workspace 结构

| crate | 职责 |
|---|---|
| gw-proto | 与云侧对齐的报文模型/主题构造 |
| gw-core | 纯领域逻辑：outbox 语义、能量单调、时钟安全、指令解释（零 IO） |
| gw-store | SQLite(WAL)：配置/电表档案/outbox/读数/日账/日志 |
| gw-collector | MeterTransport 端口 + Modbus RTU 真驱动；寄存器档案 TOML |
| gw-link | CloudLink 端口 + rumqttc QoS1 实现 |
| gw-agent | 任务编排（采集/发布/心跳/指令/快照）；含 `gateway-headless` 无 UI 二进制 |
| gw-ui | Slint 五页面触屏 UI；`park-gateway` 主二进制 |

## 核心机制（详见 DESIGN.md §6）

- **先落库再出网**：采集→SQLite outbox→发布器，断电/断网后按原序补发，messageId 幂等去重；
- **能量单调**：读数回退（清零/换表）样本自动拦截并留痕，保障云侧计费差值正确；
- **时钟安全**：无 RTC 设备时钟未同步时数据暂存不出网，NTP 恢复后补时间戳发布；
- **指令真实执行**：READ_NOW / SET_INTERVAL / REBOOT_GATEWAY，回执与云侧状态机对齐。
- **RS485 总线模型**：一条 485 通道可挂多台 Modbus RTU 设备，以 `channel_id + modbus_addr` 唯一定位；
- **竖屏适配**：系统保持原生竖屏，Web UI 可通过 `display-config.json` 在竖屏内显示横向操作界面。

## 部署验证

```bash
cargo check --workspace
npm --prefix web-ui run build
```
