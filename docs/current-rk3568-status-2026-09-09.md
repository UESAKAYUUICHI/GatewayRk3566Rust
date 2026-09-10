# RK3568 网关当前现状记录

检查时间：2026-09-09 10:19 CST  
连接方式：本地 SSH 连接 `root@10.220.166.62`  
设备主机名：`AiGateway`

## 结论摘要

当前 RK3568 网关板已正常在线，网关服务、Web 前端、全屏 kiosk 浏览器和 NetworkManager 均处于运行状态。系统真实 Wi-Fi、网关后端 API、浏览器页面显示已对齐，当前连接均为 `REDMI K90 Ultra`。

主要状态如下：

- 系统：Ubuntu 24.04.4 LTS，Linux 6.1.141，arm64
- CPU：4 核 Cortex-A55
- 内存：总计 3.8 GiB，可用约 2.9 GiB
- Swap：4.0 GiB，当前未使用
- 根分区：29 GiB，总使用 13 GiB，剩余约 15 GiB
- Wi-Fi：`wlan0` 已连接 `REDMI K90 Ultra`
- IP：`10.220.166.62/24`
- 网关服务：`park-gateway.service` active，并已 enabled
- Web 服务地址：`0.0.0.0:8080`
- 本机健康接口：正常
- 浏览器 kiosk：Chromium 已全屏打开网关页面
- 锁屏器：未运行

## 系统环境

设备基础信息：

```text
Hostname: AiGateway
OS: Ubuntu 24.04.4 LTS
Kernel: Linux 6.1.141
Architecture: aarch64 / arm64
CPU: 4 x Cortex-A55
Boot time: 2026-03-24 21:45
Uptime: up 1 hour, 26 minutes
```

资源情况：

```text
Memory:
  total: 3.8 GiB
  used: 893 MiB
  free: 1.0 GiB
  available: 2.9 GiB

Swap:
  /swapfile: 4.0 GiB
  used: 0 B

Disk:
  /dev/root ext4
  size: 29 GiB
  used: 13 GiB
  available: 15 GiB
  usage: 45%
```

当前资源余量够跑网关服务、Chromium kiosk 和 Rust 后端。之前板端 release 编译 `gw-web` 时内存没有明显压力，Swap 也未被使用。

## 网络状态

系统真实网络状态：

```text
wlan0 UP 10.220.166.62/24
default via 10.220.166.241 dev wlan0
DNS: 10.220.166.241
```

NetworkManager 当前连接：

```text
REDMI K90 Ultra
type: 802-11-wireless
device: wlan0
state: activated
```

Wi-Fi 扫描中当前连接项：

```text
REDMI K90 Ultra
signal: 59%
security: WPA2
connected: yes
```

附近还可见 `Laplace_5G`、`Laplace`、`TP-LINK_AA42`、`WZUSP` 等 Wi-Fi，但它们不是当前连接。

## 网关程序状态

Systemd 服务：

```text
park-gateway.service: active
enabled: enabled
main process: /usr/bin/gateway-web
bind: 0.0.0.0:8080
```

健康接口：

```json
{"status":"ok","version":"0.1.0","cloudOnline":true}
```

网关快照中的运行模式：

```json
{
  "production": true,
  "transport": "modbus-rtu",
  "cloudLink": "mqtt-qos1",
  "network": "networkmanager-nmcli"
}
```

网关 API 返回的 Wi-Fi 状态：

```json
{
  "connected": true,
  "ssid": "REDMI K90 Ultra",
  "signal": 59,
  "ipv4": "10.220.166.62/24",
  "gateway": "10.220.166.241",
  "dns": "10.220.166.241",
  "interfaceName": "wlan0"
}
```

当前平台同步到本地的电表数量为 2 台，均绑定在 `rs485-1`：

- `WZBC-LS2-M-325`
- `WZBC-LS2-M-326`

当前快照中两台电表均未采到在线读数，页面显示为离线/空值。这更像是 RS485 现场设备、接线、地址、波特率或仪表响应问题，不是 Web 服务本身的问题。

## RS485 与串口

系统当前可见串口设备：

```text
/dev/ttyS0
/dev/ttyS3
/dev/ttyS4
/dev/ttyS7
/dev/ttyS8
```

网关配置中当前 RS485 通道使用：

```text
port: /dev/ttyS3
baud: 9600
data_bits: 8
stop_bits: 1
parity: NONE
```

服务日志显示 `rs485-1` 已建立，端口为 `/dev/ttyS3`。

## 前端与 Kiosk

当前桌面与浏览器状态：

```text
LightDM: active
Desktop session: kickpi / X11 / :0
LockedHint: no
Chromium kiosk: running
Page URL: http://127.0.0.1:8080/#/overview/dashboard
```

Chromium kiosk 主要配置：

- 全屏 kiosk 模式
- 禁用翻译弹窗
- 禁用常见返回/刷新/下拉手势
- 禁用缩放手势
- 使用 `--password-store=basic`，降低密钥环密码弹窗概率
- 开启本机调试端口 `127.0.0.1:9222`
- 关闭 GPU/视频/Canvas 加速，绕开 RK 图形栈导致的 Chromium 崩溃

自动登录与自启动文件位于：

```text
/home/kickpi/.config/autostart/park-gateway-kiosk.desktop
/home/kickpi/.config/autostart/park-gateway-auto-unlock.desktop
/home/kickpi/.config/autostart/xfce4-screensaver.desktop
/home/kickpi/.config/autostart/light-locker.desktop
```

当前未发现 `xfce4-screensaver`、`light-locker`、`xscreensaver`、`gnome-screensaver` 进程。

## 已完成的重要修复

### Wi-Fi 显示旧状态问题

之前用户看到板子实际连接 `REDMI K90 Ultra`，但前端页面显示 `Laplace_5G`。排查后确认：

- 系统真实连接是 `REDMI K90 Ultra`
- 后端 HTTP API 返回也是 `REDMI K90 Ultra`
- 浏览器页面中 Vue 状态停留在旧 WebSocket 快照，显示 `Laplace_5G`

根因是 WebSocket 推送只刷新采集动态字段，没有重新读取 Wi-Fi 状态。已修复：

- `gw-network` 增加缓存 Wi-Fi 列表读取能力
- `gw-web` 的 WebSocket 推送更新时同步刷新 Wi-Fi 状态
- 避免旧 WebSocket 快照覆盖前端当前网络状态

当前已验证页面显示：

```text
wlan0 · REDMI K90 Ultra
10.220.166.62/24
```

### Chromium 白屏问题

之前重启后 Chromium 出现全屏白屏。排查发现：

- 网关服务返回 HTML、JS、CSS 都正常
- Chromium 窗口存在
- DevTools 中页面上下文曾停留在 `about:blank`

修复方式：

- 不再使用 `--app=http://127.0.0.1:8080/`
- 改为直接用 `--kiosk http://127.0.0.1:8080/` 打开页面

当前 DevTools target 已正常指向：

```text
http://127.0.0.1:8080/#/overview/dashboard
```

### Chromium 崩溃问题

之前 Chromium 几秒后闪退，日志出现：

```text
GPU process isn't usable. Goodbye.
```

根因与 RK 图形栈和 Chromium GPU 进程有关，同时曾经的启动参数误禁用了软件渲染兜底。当前已通过禁用 GPU 加速和保留软件渲染方式稳定运行。

### 锁屏密码问题

LightDM 自动登录已经生效，但曾经出现登录后密码框。当前处理：

- 禁用/屏蔽常见锁屏器自启动
- kiosk 启动时主动杀掉锁屏器
- 增加 `park-gateway-auto-unlock`，在早期桌面阶段尝试输入本机密码 `kickpi`
- Chromium 使用 `--password-store=basic`，减少密钥环弹窗

当前检查未发现锁屏器进程，`LockedHint=no`。

## 当前已知风险

### MQTT 日志仍有网络异常记录

服务日志中近期出现过 MQTT 链路异常，例如：

```text
Last pingreq isn't acked
No route to host
Network timeout
Network is unreachable
```

但当前健康接口返回 `cloudOnline: true`。这些日志可能来自 Wi-Fi 切换、网络短暂断开、路由暂不可达或平台 MQTT 连接瞬时波动。建议后续继续观察平台侧是否稳定收到心跳和数据。

### RS485 设备未在线

当前已有 2 台平台下发电表配置，但快照中读数为空、在线状态为 false。下一步应重点确认：

- RS485 A/B 接线是否正确
- 仪表地址是否分别为 1 和 2
- `/dev/ttyS3` 是否对应实际 485 口
- 仪表通讯参数是否为 `9600 8N1`
- 总线终端电阻、接地和线序是否可靠

### 前端仍有编码显示问题

SSH 中看到中文日志/HTML 部分显示为乱码，主要是终端编码显示问题，不一定代表浏览器界面乱码。浏览器截图此前显示中文界面可正常渲染。

## 当前文件部署状态

关键部署文件：

```text
/usr/bin/gateway-web        8.6M
/usr/bin/gateway-headless   5.2M
/usr/share/park-gateway/web-ui/index.html
/etc/park-gateway/gateway.toml
/usr/local/bin/park-gateway-kiosk
/usr/local/bin/park-gateway-auto-unlock
```

前端文件数量约 519 个。

## 建议下一步

1. 现场确认屏幕上 Wi-Fi 当前连接显示是否为 `REDMI K90 Ultra`。
2. 观察一次重启后的桌面是否还会弹密码框。
3. 用真实 RS485 仪表做通讯验证，优先确认 `/dev/ttyS3`、地址 1/2、9600 8N1。
4. 连续观察 MQTT 平台在线状态，确认网络切换后是否仍有 `Network is unreachable` 告警。
5. 如果后续要正式交付，建议增加一个“系统诊断页”，直接显示系统真实 Wi-Fi、网关 API Wi-Fi、WebSocket Wi-Fi 三者，避免现场误判。
