# RK3568 园区能源网关（Rust + Slint/Vue）设计文档

> 版本：v2.0（2026-08-30）
> 定位：作为烧录进 RK3568 真实设备的网关程序；
> 向下经 RS485（Modbus RTU）采集真实电表，向上与 `park-energy-access` 严格协议兼容。
> 云侧三个后端服务与 86 表模型**不改代码**（唯一建议动作：EMQX 开启认证，启用 `dev_gateway.mqtt_secret`）。

## 1. 目标与非目标

**目标**

1. 与现有链路协议级兼容：主题、报文字段、messageId 规则、QoS1、collectTime 语义全部对齐云侧校验逻辑；
2. 数据不丢不乱序：断电 / 断网 / 进程崩溃后，报文按原始顺序补发且读数单调；
3. 高复用：核心域（协议、管道、指令语义）零 IO 依赖；生产驱动全部走真实 Modbus RTU、MQTT 与 NetworkManager；
4. 高灵活：电表接入档案化（寄存器映射是数据不是代码）、配置三层合并（内置默认 < TOML 引导 < 数据库运行时）；
5. 高性能：常驻内存目标 < 60MB，UI 渲染 GPU 加速，采集→出网全链路无阻塞 IO；
6. 可实际部署：aarch64 交叉编译、systemd + 看门狗、Wayland kiosk、deb 打包，落盘路径符合 FHS。

**非目标**：边缘侧本地计费/告警（云端已有）；OTA 升级（预留方案，不在首版实现）；多网关级联。

## 2. 与云侧链路的兼容契约

| 约束 | 云侧来源 | 网关侧对策 |
|---|---|---|
| 主题 `gateway/{id}/data/upload` `status/heartbeat` `cmd/response`；下行 `cmd/down` | MqttTopicParser 正则 | 主题构造器集中在 gw-proto |
| 报文字段：schemaVersion/messageId/gatewaySn/timestamp/type/meters[]{deviceSn, modbusAddr, collectTime, quality, points} | GatewayUploadPayload / MeterPayload | gw-proto serde 结构体逐字段对齐 |
| messageId = `MSG-{gatewayId}-{ts}` / `HB-…`，且为幂等键 | isDuplicate(gatewayId+messageId) | 入 outbox 时定稿且 UNIQUE，重放复用 |
| collectTime 未来 >10min 或超 90 天拒收 | validateQuality | 时钟不可信时不出网（§6.3） |
| 心跳 30s 无 → 离线 | GatewayStatusService | 心跳周期 10s（阈值 1/3） |
| 未注册 deviceSn 进 discovered_device、不入数据流 | inspectMeter | 配置界面 SN 校验 + 提示 |
| quality≠0 样本记 invalid | validateQuality | 正常恒 0；缺采不上报（交给云侧断采告警）；可疑读数才标非 0 |
| 设备限流 120 报/分 | GatewayDeviceRateLimiter | 配置校验：采集间隔 ≥ 1s（默认 300s） |
| 指令体 {commandId, targetType, targetId, targetSn, commandType, payload}；回执只认 commandId+status | CommandService / CommandResponsePayload | gw-proto 定义同构结构 |
| 指令类型 READ_NOW / SET_INTERVAL / REBOOT_GATEWAY | PlatformBusinessQueryService.commandTypes | 真实执行（§6.5） |

## 3. 总体架构：六边形 + 分层

```
                    ┌──────────────────────────────┐
   7寸屏 1024×600   │  Slint UI (gw-ui)             │  主线程事件循环
                    │  页面/模型/回调  ← 无任何IO    │
                    └──────┬────────────▲──────────┘
                 UserCommand(mpsc)   Snapshot(watch, 1Hz合拍)
                    ┌──────▼────────────┴──────────┐
                    │  Agent 编排层 (gw-agent)       │  tokio 运行时
                    │  采集调度/发布器/心跳/指令执行/  │
                    │  健康状态机/看门狗feed          │
                    └──┬─────────┬─────────┬────────┘
                       │端口(trait)│         │
        ┌──────────────▼──┐  ┌───▼─────┐  ┌▼────────────┐
        │ MeterTransport  │  │CloudLink│  │ MessageStore │  ← 驱动/适配层
        │ └ ModbusRtu    │  │ └ Mqtt  │  │ ├ Sqlite(真) │
        │                 │  │         │  │               │
        └─────────────────┘  └─────────┘  └─────────────┘
              gw-core（纯领域：报文构造、outbox 语义、能量规则、指令语义、
              时钟安全策略——只依赖标准库与 serde，全部可 PC 单测）
```

要点：

- **依赖方向单向**：gw-ui / 驱动层 → gw-agent → gw-core；gw-core 不依赖任何 IO crate，Slint/rumqttc/rusqlite 永不出现在 core。
- **可替换性即复用性**：换 MQTT broker 实现、换Modbus 库、把 SQLite 换内存库跑 CI，都不动核心逻辑；生产服务由 `gateway-web` 常驻，`gateway-headless` 用于无 Web 的长跑和交叉编译冒烟。
- UI 与 agent 只通过两个通道通信（命令下行 mpsc、状态上行 watch），UI 卡死不影响采集上报。

## 4. Workspace 划分

当前仓库为单一空 crate（edition 2024），改造为 workspace（包名统一小写，目录名不变）：

```
GatewayRk3566Rust/
├─ Cargo.toml                 # [workspace] 虚拟清单 + 共享 [workspace.dependencies]
├─ crates/
│  ├─ gw-proto/               # 报文模型/编解码/主题构造
│  ├─ gw-core/                # 纯领域逻辑（无 IO）
│  ├─ gw-store/               # MessageStore/ConfigStore: rusqlite(WAL) 实现
│  ├─ gw-collector/           # MeterTransport trait + ModbusRtu(tokio-modbus)
│  ├─ gw-link/                # CloudLink trait + Mqtt(rumqttc)
│  ├─ gw-agent/               # tokio 任务编排、依赖注入装配、headless bin
│  └─ gw-ui/                  # Slint 界面（编译 .slint，暴露 GatewayWindow）
│     └─ gateway.bin          # 主二进制：装配 agent + 启动 Slint 事件循环
├─ ui/                        # .slint 文件与设计令牌（颜色/字号/间距）
├─ profiles/                  # 电表寄存器档案（PD666-3S3.toml 等，数据不是代码）
├─ config/gateway.toml        # 引导配置示例
├─ deploy/                    # systemd×2、udev、cross 脚本、deb、安装脚本
└─ docs/DESIGN.md             # 本文档
```

共享依赖版本集中在 workspace 级（tokio、serde、tracing…），避免 crate 间版本漂移。

## 5. 存储（SQLite，WAL）

数据目录 `/var/lib/park-gateway/gateway.db`；单写连接 + WAL，读走同一连接（嵌入式单进程足够）。表：

```sql
gateway_config(key TEXT PRIMARY KEY, value TEXT);          -- 运行时配置(含云端身份/间隔)
meter(id INTEGER PRIMARY KEY, device_sn TEXT UNIQUE NOT NULL,
      modbus_addr INT NOT NULL, profile TEXT NOT NULL DEFAULT 'PD666-3S3',
      collect_interval_s INT NOT NULL DEFAULT 300, enabled INT NOT NULL DEFAULT 1, created_ms INT);
meter_register_map(profile TEXT, point_code TEXT, func INT, address INT, quantity INT,
                   scale REAL, offset REAL, PRIMARY KEY(profile, point_code));   -- 档案来自 profiles/*.toml 导入
outbox_message(id INTEGER PRIMARY KEY AUTOINCREMENT, message_id TEXT UNIQUE NOT NULL,
               topic TEXT NOT NULL, payload BLOB NOT NULL,       -- 入队即序列化定稿，重放零重编码
               created_ms INT NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
               attempts INT NOT NULL DEFAULT 0, last_error TEXT);
meter_reading(meter_id INTEGER PRIMARY KEY REFERENCES meter(id), snapshot TEXT NOT NULL,
              read_ms INT NOT NULL, quality INT NOT NULL, updated_ms INT NOT NULL);
energy_daily(meter_id INT, day TEXT, delta_kwh REAL, PRIMARY KEY(meter_id, day));
command_log(id INTEGER PRIMARY KEY AUTOINCREMENT, command_id TEXT UNIQUE NOT NULL,
            command_type TEXT, target_sn TEXT, payload TEXT,
            received_ms INT, result_status TEXT, responded_ms INT, message TEXT);
event_log(id INTEGER PRIMARY KEY AUTOINCREMENT, ts_ms INT, level TEXT, source TEXT, message TEXT);
```

写入模式：outbox 只 INSERT/UPDATE status（append-only 语义）；reading/energy_daily 为 UPSERT；sent 行保留 N 天由每日清理任务删除（防无限增长）。

## 6. 核心机制（gw-core 承载）

### 6.1 Outbox 管道
`Poller → 组报文(messageId定稿) → INSERT pending → notify → Publisher 按 id 序 QoS1 发布 → PUBACK → UPDATE sent`。读数快照随报文一并定稿入库，重放原样重发，从机制上杜绝断链期间计数器照加、重放后能量乱序的问题。心跳不入 outbox。

### 6.2 幂等接力
messageId 入队定稿 + 云侧 isDuplicate → at-least-once 传输在应用层等效 exactly-once。

### 6.3 时钟安全（无 RTC 电池）
开机若系统时间早于固件构建时间 → 判定时钟不可信：采集照常、入 outbox，**不发布**；systemd-timesyncd 同步后统一补时间戳 flush。累积量计量天然容忍采样点后移，对计费差值无影响。

### 6.4 采集与能量单调
同串口半双工按表轮询（deadline 调度，单表超时重试、连续失败标离线）；读数回退（清零/换表）该样本不上报并写 event_log；寄存器映射走档案表，新表型零代码接入。

### 6.5 指令执行
READ_NOW → 立即读目标设备单发一帧；SET_INTERVAL → 写 gateway_config 持久化；REBOOT_GATEWAY → 回执 SUCCESS 后延迟 `systemctl reboot`；未知类型 → FAILED+message。回执 `{commandId, status, message, timestamp}`。

### 6.6 身份
MQTT username = gatewaySn，password = 平台 `dev_gateway.mqtt_secret`（EMQX 认证数据源启用后即生效，云代码不动）。

## 7. Slint UI 设计

### 7.1 技术选型
slint 1.x（GPLv3 免费用于毕设；若将来商业需 Royalty-Free License）。渲染：`backend-winit + renderer-femtovg`（RK3566 Mali-G52 + Panfrost → GLES 可用）；保留 `renderer-software` 作为无 GPU 驱动时的兜底 feature。样式基于 Material/Fluent 派生的工业风设计令牌（大字号、高对比、≥48px 触控目标）。

### 7.2 线程模型（关键正确性设计）
```
主线程:   Slint run_event_loop()（部分平台要求 UI 在主线程）
工作线程: tokio Runtime::new()（多线程池）承载 agent 全部任务
UI→核心:  mpsc::channel<UserCommand>()     （页面回调 → send，绝不阻塞）
核心→UI:  tokio::sync::watch::channel<Snapshot>()
          UI 适配任务 1Hz 合拍 → slint::Weak::upgrade_in_event_loop 更新属性/模型
```
规则：Slint 回调里只允许 `try_send`；agent 永不持有 UI 句柄——UI 崩溃可被 systemd 拉起而不丢采集。

### 7.3 页面（1024×600，左侧图标导航 5 项）
1. **状态首页**：云连接/时钟同步/串口健康/出网积压(outbox 深度)/在线表数/今日用电/最近上报时间；
2. **电表实时**：逐表卡片（三相电压/电流/功率/功率因数/正向有功电能），watch 通道刷新；
3. **电表配置**：列表 + 编辑器（SN、Modbus 地址、档案、采集周期、启停；含 SN 规则与最小间隔校验）；
4. **通信设置**：MQTT 地址/端口/凭据、云端网关 ID/SN、链路诊断按钮（结果走事件通道回显）；
5. **日志**：指令记录 / 事件日志两个 Tab（模型分页，环形式保留最近 500 条在内存，全量看库）。

### 7.4 复用性
.slint/Vue 页面与 Rust 逻辑分离：页面只声明属性/模型/回调，全部业务语义在 gw-agent；`gateway-web` 是生产主服务，`gateway-headless` bin 不链接前端资源，可用于无 Web 的长跑。

## 8. 配置体系（三层合并）

```
内置默认(代码) < /etc/park-gateway/gateway.toml(引导: MQTT地址/云端ID/SN/心跳周期)
              < gateway_config 表(运行时可变: 采集间隔/电表清单/SET_INTERVAL 结果)
```
优先级高者覆盖低者；每次合并结果进 event_log 便于追溯。引导配置缺失时 UI 引导页兜底配置（首次开机可用性）。

## 9. 性能设计

| 指标 | 预算 | 手段 |
|---|---|---|
| 常驻内存 | < 60MB | 无 GC、模型节流、outbox SQL 分页、Slint 而非浏览器（对比 kiosk 方案省 ~300MB） |
| 采集周期抖动 | < 50ms | tokio deadline 调度、串口独占队列 |
| 入队→PUBACK | < 10ms（本地 broker） | payload 入队即定稿 BLOB，重放零重编码；rumqttc 有界通道 |
| UI 刷新 | 1Hz 合拍 | watch 通道天然去抖，upgrade_in_event_loop 聚合更新 |
| 冷启动到 UI | < 15s | release + LTO("thin") + strip；Slint 声明式编译期生成代码 |
| 磁盘 | WAL + sent 行 7 天滚动清理 | 每日维护任务 |

## 10. 部署方案（Linux 实机）

1. **构建**：x86 主机 `cross build --release --target aarch64-unknown-linux-gnu`（Ubuntu glibc 直接可跑）；板上原生 cargo 亦可。rust-toolchain.toml 固定版本。
2. **打包/安装**：安装 `gateway-web`、`gateway-headless`、`park-gateway`、`web-ui/dist`、profiles/、systemd 单元、udev 规则、polkit 规则，postinst 建目录授权。
3. **显示**：生产优先使用浏览器访问 `gateway-web` 托管的 Web UI；如需本机屏幕，可用浏览器 kiosk 指向 `http://127.0.0.1:8080/`。
4. **服务**：
   - `park-gateway.service`：运行 `gateway-web`，Restart=always，After=network-online timesyncd。
   - 串口 udev -> dialout 组；Wi-Fi 管理通过 NetworkManager + polkit 授权 `parkgw`；`/var/lib/park-gateway/`（数据）与 `/etc/park-gateway/`（配置）分离。
5. **日志**：tracing → journald（systemd 原生）+ 每日滚动文件（event_log 表已含结构化事件）。
6. **联调脚本**：deploy/ 提供 cloud-echo（本地 mosquitto_sub 订阅全主题的验收脚本）与断链长跑脚本。

## 11. 实机验收策略

| 层 | 覆盖 |
|---|---|
| 云侧协议 | 心跳、数据上报、指令回执、告警上报均进入 `park-energy-access` |
| 采集链路 | RS485 真表读取、能量单调、断采事件、恢复后继续采集 |
| 出网链路 | 断网积压、恢复补发、MQTT QoS1、messageId 幂等 |
| 网络链路 | NetworkManager 扫描、连接、断开、忘记 Wi-Fi |
| 稳定性 | 72 小时运行，内存稳定，进程异常退出后 systemd 自动拉起 |

## 12. 里程碑

- **M1 骨架与协议**：workspace 改造；gw-proto；与本地 access 联调一条上行数据落库（验证"云不改"）。
- **M2 存储与管道**：gw-store + outbox；断电/断网重放机制。
- **M3 驱动与通信**：ModbusRtu 真驱动（先 USB-485 + PD666-3S3 实表）；MQTT 真链接入云 EMQX；指令真实执行。
- **M4 UI**：Slint 五页面 + 双通道接线 + headless bin。
- **M5 上机**：交叉编译、cage kiosk、systemd/看门狗、deb、72h 长跑与论文素材记录。

## 13. 风险与对策

| 风险 | 对策 |
|---|---|
| Panfrost/GLES 驱动不稳 | software renderer feature 一键切换（性能足够 1Hz 仪表盘） |
| Slint License | 毕设 GPL 合规；文档注明，不阻塞 |
| tokio-modbus 与部分表兼容性 | MeterTransport trait 隔离，必要时换库只动 gw-collector |
| 无 RTC 导致 collectTime 漂移 | §6.3 时钟安全策略 + timesyncd 状态探测 |
| outbox 无限增长 | sent 7 天滚动清理 + 磁盘水位事件告警 |
| 云侧 EMQX 未开认证 | 首版可用匿名直连跑通，认证作为部署项单独开关 |

