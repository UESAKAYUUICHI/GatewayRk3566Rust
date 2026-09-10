# 真实网关化调整记录

本文记录当前项目从演示原型推进到 RK3568 现场网关时已经落地的关键改动。

## 1. RS485 总线模型

RS485 不是“一条串口只能接一台设备”，而是一条半双工总线。一条通道可以挂载多台 Modbus RTU 从站，网关按 `channel_id + modbus_addr` 定位设备。

当前规则：

- 同一 RS485 通道允许绑定多台设备。
- 同一 RS485 通道内，`modbus_addr` 必须唯一，范围为 `1..=247`。
- 暂存区 `__staging__` 不参与采集，因此不限制 Modbus 地址重复。
- 同一通道共享一条串口连接和一把异步锁，保证总线访问严格串行。
- 不同 RS485 通道可并行采集。

涉及实现：

- `gw-store` 数据库唯一索引改为真实通道内的 `channel_id + modbus_addr` 唯一。
- 旧的“同一通道只保留一台启用设备”的迁移已改为占位，不再挪走设备。
- 绑定暂存设备时不再踢掉原设备，只检查目标总线地址是否重复。
- Web API 保存本地设备时按 `channel_id + modbus_addr` 做冲突校验。
- 前端设备页面改为按通道展示多台设备和地址列表。

## 2. 串口参数真实生效

通道页面可配置的串口参数已经传入真实 Modbus RTU 驱动：

- `port`
- `baud`
- `data_bits`
- `stop_bits`
- `parity`

默认配置位于 `config/gateway.toml`：

```toml
[serial]
port = "/dev/ttyS3"
baud = 9600
data_bits = 8
stop_bits = 1
parity = "NONE"
```

现场更换电表或 485 转换器后，应优先确认这些参数与设备说明书一致。

## 3. Wi-Fi 真实控制

网络模块使用板端 `NetworkManager` 的 `nmcli` 作为真实控制入口：

- 扫描 Wi-Fi
- 连接指定 SSID
- 断开当前 Wi-Fi
- 忘记已保存网络
- 读取接口、SSID、信号、IPv4、网关、DNS

部署脚本会安装 `deploy/49-park-gateway-networkmanager.rules`，授权服务用户 `parkgw` 调用 NetworkManager。板端需要保证：

- `NetworkManager.service` 正常运行。
- `nmcli` 可用。
- `parkgw` 用户存在，并在 `netdev` 组内。
- polkit 规则已放入 `/etc/polkit-1/rules.d/`。

## 4. 竖屏设备上的横向前端

系统层屏幕保持原生竖屏，不再做 XRandR/DRM 层旋转。前端通过静态配置决定显示方向。

配置文件：

```json
{
  "mode": "landscape-in-portrait",
  "designWidth": 1280,
  "designHeight": 800
}
```

文件位置：

- 开发时：`web-ui/public/display-config.json`
- 打包后：`web-ui/dist/display-config.json`
- 板端部署后：`/usr/share/park-gateway/web-ui/display-config.json`

模式说明：

- `native-portrait`：页面按屏幕原始方向显示。
- `native-landscape`：页面按浏览器原始横屏显示。
- `landscape-in-portrait`：在 800x1280 竖屏里把 1280x800 的横向操作界面由前端旋转显示，不改系统显示方向。

这正适合当前触摸问题：系统和 Goodix 触摸仍保持原生竖屏坐标，业务页面自己适配横向视觉。

## 5. 部署前验证

本地已通过：

```bash
cargo check --workspace
npm --prefix web-ui run build
```

板端恢复网络后建议执行：

```bash
systemctl restart park-gateway.service
systemctl --no-pager status park-gateway.service
journalctl -u park-gateway -f
```

然后在 Web UI 里确认：

- 串口通道为板端真实设备，例如 `/dev/ttyS3`。
- 每台 485 设备的 Modbus 地址不重复。
- Wi-Fi 扫描和连接能返回真实状态。
- `/usr/share/park-gateway/web-ui/display-config.json` 中的显示模式符合现场屏幕。
