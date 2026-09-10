# 部署到 RK3568（Ubuntu 22.04）实操手册

## 0. 目标板假设
- RK3568（4×A55，≥1GB RAM），Ubuntu 22.04 aarch64
- 7 寸 DSI/MIPI 屏（Wayland，Panfrost/GLES 可用）
- 串口：按实际板端设备配置，可用普通 UART（如 `/dev/ttyS0`、`/dev/ttyS1`）或外接 USB-485（如 `/dev/ttyUSB0`）
- 网络：以太网或 Wi-Fi（NetworkManager/nmcli 可用，能与云服务器 1883 端口互通）

## 1. 开发机构建（x86 Windows/Linux 均可）
```bash
# 方式一：cross（推荐，Docker 容器内自动交叉编译）
cargo install cross
MODE=release TARGET=aarch64-unknown-linux-gnu ./deploy/build-arm64.sh

# 方式二：原生 cargo（Linux 主机 + aarch64 工具链）
rustup target add aarch64-unknown-linux-gnu
MODE=release ./deploy/build-arm64.sh
```

## 2. 真机联调
```bash
# 板端直接连真实 RS485、真实 MQTT、真实 NetworkManager
gateway-web --config /etc/park-gateway/gateway.toml --web-root /usr/share/park-gateway/web-ui --bind 0.0.0.0:8080

# 验收脚本：订阅全部网关主题观察心跳/数据/回执
deploy/cloud-echo.sh
```
云侧验收清单（对应 park-energy-access）：
- `log_raw_message` 出现 DATA_UPLOAD 且状态 FORWARDED
- `dev_gateway.online_status` 变在线（心跳 10s < 30s 阈值）
- `dev_device`/`data_ingest_event` 出现对应设备与事件
- 平台页面「接入诊断/设备控制台」下发 READ_NOW → 网关真读一次表并回执

## 3. 板端安装
```bash
# 拷贝产物与 deploy/ 到板子后（scp 或 U 盘）
sudo ./deploy/install-on-device.sh
```
服务以 `gateway-web` 常驻运行，同时提供采集、发布、接收指令、Web UI 和 Wi-Fi 管理；`Restart=always` 与
`ProtectSystem=strict` 已启用（数据仅 `/var/lib/park-gateway` 可写）。
当前程序尚未实现 systemd 看门狗协议，因此不设置 `WatchdogSec`，避免健康进程被误重启。

## 4. Wi-Fi 权限

安装脚本会创建 `parkgw` 系统用户，加入 `dialout/netdev`，并安装 polkit 规则授权该用户调用
NetworkManager。Web UI 的扫描、连接、断开和忘记网络均由板端 `nmcli` 执行。

## 5. 时钟与 NTP（无 RTC 电池必读）
```bash
sudo timedatectl set-ntp true
timedatectl status   # 确认 "System clock synchronized: yes"
```
时钟未同步时网关自动不出网（数据暂存 SQLite deferred 行），同步后补时间戳发布——这是与云侧
`DataIngestService.validateQuality`（拒收未来>10min/超90天数据）对齐的边缘侧保护。

## 6. 运行时配置
- 电表/通信参数：网关 Web UI「设备采集 / 采集通道」页面，串口设备路径可自定义，写 SQLite，实时生效
  （broker 地址等连接参数重启服务生效）
- 预设：`/etc/park-gateway/gateway.toml`（首次启动种子）
- 档案：`/usr/share/park-gateway/profiles/*.toml`（新表型零代码接入）

## 7. 验证与排障
```bash
journalctl -u park-gateway -f          # 服务日志
sqlite3 /var/lib/park-gateway/gateway.db \
  "SELECT status, COUNT(*) FROM outbox_message GROUP BY status;"   # 出网积压
ls -l /dev/rs485-usb                    # udev 规则生效？
sudo -u parkgw /usr/bin/gateway-web --config /etc/park-gateway/gateway.toml --web-root /usr/share/park-gateway/web-ui --bind 0.0.0.0:8080
```

## 8. 72 小时长跑验收清单
- [ ] RSS 内存稳定（`watch systemctl status park-gateway`），无持续增长
- [ ] outbox：sent 行 7 天滚动清理，pending 长期为 0
- [ ] 拔网线 30 分钟 → outbox 积压 → 恢复后按序补发，云侧无重复入库（messageId 幂等）
- [ ] 拔 RS485 → 电表判离线 + 事件留痕 → 重插恢复
- [ ] `systemctl kill -s SIGKILL park-gateway` → 3s 内自动拉起，数据无丢失
- [ ] 云侧平台页面数值与表计实读一致
