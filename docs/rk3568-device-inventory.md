# RK3568 Ubuntu 开发板设施盘点

盘点时间：2026-09-07，更新于 2026-09-08  
连接方式：本机 SSH 连接 `root@10.220.166.62`，旧地址为 `192.168.123.105`  
设备用途判断：RK3568 AI/视觉网关开发环境

本文档基于对真实 RK3568 开发板的 SSH 检查和部署前整理。初始盘点为只读检查；后续已完成 swap、网络、基础工具链和屏幕电源策略等配置调整。

## 1. 设备概览

这台设备是一块基于 Rockchip RK3568 的开发板，设备树标识为：

- `rockchip,rk3568-kickpi-k1a`
- `rockchip,rk3568`

系统主机名为 `AiGateway`，运行 Ubuntu 24.04.4 LTS arm64，内核为 Rockchip 定制的 Linux 6.1.141。系统包含桌面环境、摄像头 ISP、RGA、MPP、RKNN runtime 等组件，整体更接近“AI 视频网关/视觉应用开发镜像”，而不是最小化服务器系统。

## 2. 系统环境

| 项目 | 当前值 |
| --- | --- |
| 主机名 | `AiGateway` |
| 操作系统 | Ubuntu 24.04.4 LTS |
| 版本代号 | Noble Numbat |
| 架构 | `arm64` / `aarch64` |
| 内核 | `Linux 6.1.141` |
| 构建信息 | `RK_BUILD_INFO="root@oranth Fri Apr 10 17:20:01 CST 2026"` |
| 当前登录用户 | `root` |
| 普通桌面用户 | `kickpi` |

系统当前启动时间较短，检查时运行约 16 分钟。日志显示设备启动后曾通过网络时间服务把日期从 `2026-03-24` 校正到 `2026-09-07`，说明设备本地 RTC 或初始系统时间可能不可靠，联网校时后才准确。

## 3. CPU 与内存

### CPU

| 项目 | 当前值 |
| --- | --- |
| CPU 架构 | ARMv8 AArch64 |
| 核心数量 | 4 |
| 核心型号 | Cortex-A55 |
| 线程 | 每核心 1 线程 |
| 最高频率 | 1992 MHz |
| 最低频率 | 408 MHz |
| 当前调度策略 | `schedutil` |

检查时 CPU 频率处于动态调节状态，多个核心频率在 1.4 GHz 到 1.992 GHz 左右变化。这说明 CPU 频率调节功能正常。

### 内存

| 项目 | 当前值 |
| --- | --- |
| 总内存 | 3.8 GiB |
| 已用内存 | 约 670 MiB |
| 可用内存 | 约 3.2 GiB |
| Swap | 已启用 4 GiB `/swapfile` |

当前内存余量比较充足。已额外配置 4 GiB swap，用于缓冲 Rust/Node 构建、桌面会话和偶发内存峰值；不建议把它当作长期高负载内存扩展使用。

## 4. 存储与分区

设备主存储为 `mmcblk0`，容量约 29.1 GiB，存在多个 Rockchip 常见分区。

| 分区 | 大小 | 文件系统 | 挂载点 | 用途判断 |
| --- | ---: | --- | --- | --- |
| `/dev/mmcblk0p8` | 28.7 GiB | ext4 | `/` | 根文件系统 |
| `/dev/mmcblk0p6` | 128 MiB | ext4 | `/oem` | OEM/厂商配置区 |
| `/dev/mmcblk0p7` | 32 MiB | ext4 | `/userdata` | 用户数据小分区 |
| `/dev/mmcblk0p1` - `/dev/mmcblk0p5` | 4 MiB 到 128 MiB | 未显示 | 未挂载 | 启动链、内核或固件相关分区 |

根分区当前状态：

| 项目 | 当前值 |
| --- | --- |
| 总大小 | 29 GiB |
| 已用 | 5.9 GiB |
| 可用 | 22 GiB |
| 使用率 | 22% |

结论：存储空间充足，适合放置 Rust 服务二进制、配置文件、日志、模型文件和测试素材。不过 `/userdata` 只有 32 MiB，不适合放大模型或大日志；建议业务数据优先放在根分区下单独目录，例如 `/opt/aigateway`、`/var/lib/aigateway` 或 `/home/kickpi/...`。

## 5. 网络设施

### 网络接口

| 接口 | 状态 | 地址 | 说明 |
| --- | --- | --- | --- |
| `wlan0` | UP | `10.220.166.62/24` | 当前主要联网接口 |
| `end0` | DOWN / unavailable | 无 | 有线网口，当前不可用 |
| `end1` | DOWN / unavailable | 无 | 有线网口，当前不可用 |
| `lo` | UNKNOWN | `127.0.0.1/8` | 本地回环 |

当前默认路由：

```text
default via 10.220.166.1 dev wlan0
```

DNS 服务器：

```text
10.220.166.1
```

Wi-Fi 当前连接名为新网络下的活动连接。

### 注意事项

旧网络中 `wlan0` 曾同时持有 `192.168.123.105` 和 `192.168.123.106` 两个 IPv4 地址，原因是 NetworkManager 与 `dhcpcd` 同时参与地址管理。已停用 `dhcpcd`，当前新网络下只保留一个地址 `10.220.166.62`。

## 6. 已开放服务与端口

当前监听到的主要服务：

| 端口 | 协议 | 服务 | 说明 |
| ---: | --- | --- | --- |
| 22 | TCP | OpenSSH | SSH 远程登录 |
| 21 | TCP | vsftpd | FTP 服务 |
| 5555 | TCP | adbd | ADB 网络调试服务 |
| 111 | TCP/UDP | rpcbind | RPC 端口映射 |
| 5353 | UDP | avahi-daemon | mDNS/DNS-SD |
| 123 | UDP | ntpsec | 网络时间同步 |
| 631 | TCP | cupsd | CUPS 打印服务，仅本地监听 |
| 53 | TCP/UDP | systemd-resolved | 本机 DNS stub |

### 安全观察

当前 root 账户允许密码登录，且密码较弱；同时 FTP 和 ADB 5555 也处于开放状态。如果设备只在隔离实验网络中使用，风险可控；如果接入办公网、校园网或公网边界，应尽快处理：

- 修改 root 密码
- 配置 SSH key 登录
- 禁止 root 密码登录或限制来源 IP
- 关闭不用的 FTP 服务
- 关闭不用的 ADB 5555
- 仅保留项目需要的端口

## 7. 图形、桌面与输入输出设施

系统运行了图形桌面环境，主要进程包括：

- `lightdm`
- `Xorg`
- `xfce4-session`
- `xfwm4`
- `xfdesktop`
- `xfce4-panel`
- `xfce4-terminal`

这说明设备当前不是 headless 最小系统，而是带 XFCE 桌面的完整开发镜像。适合接显示器、键鼠做本地调试，也适合用作带 GUI 工具的演示系统。

相关设备节点：

| 设备 | 说明 |
| --- | --- |
| `/dev/fb0` | framebuffer |
| `/dev/dri/card0`, `/dev/dri/card1` | DRM 显示设备 |
| `/dev/dri/renderD128`, `/dev/dri/renderD129` | DRM render 节点 |
| `/dev/mali0` | Mali GPU 设备 |
| `/dev/snd/*` | 声卡/音频设备 |

## 8. 摄像头与视频设施

系统存在多个 V4L2 视频设备：

- `/dev/video0` 到 `/dev/video9`
- `/dev/video-camera0 -> video0`
- `/dev/media0`

前几个视频节点信息如下：

| 设备 | 驱动 | 类型 |
| --- | --- | --- |
| `/dev/video0` | `rkisp_v5` | `rkisp_mainpath` |
| `/dev/video1` | `rkisp_v5` | `rkisp_selfpath` |
| `/dev/video2` | `rkisp_v5` | `rkisp_rawwr0` |
| `/dev/video3` | `rkisp_v5` | `rkisp_rawwr2` |

这些节点来自 Rockchip ISP 管线，说明摄像头/ISP 驱动栈已经加载。系统还安装了：

- `v4l2-ctl`
- `ffmpeg`
- `gst-launch-1.0`
- `libv4l-rkmpp`
- Rockchip MPP GStreamer 插件：`libgstrockchipmpp.so`

结论：设备具备做摄像头采集、视频编码、视频解码、GStreamer 流水线测试和 AI 视觉前处理的基础。

## 9. NPU、RKNN 与 AI 推理设施

### 已确认设施

| 设施 | 路径或命令 | 状态 |
| --- | --- | --- |
| NPU devfreq | `/sys/class/devfreq/fde40000.npu` | 存在 |
| NPU DRM path | `/dev/dri/by-path/platform-fde40000.npu-*` | 存在 |
| RKNN runtime | `/lib/librknnrt.so` | 存在 |
| RKNN server | `/usr/bin/rknn_server` | 存在 |
| RKNN 测试程序 | `/usr/bin/rknn_common_test` | 存在 |
| RKNN 启停脚本 | `/usr/bin/start_rknn.sh`, `/usr/bin/restart_rknn.sh` | 存在 |
| 示例模型 | `/usr/share/model/RK3566_RK3568/mobilenet_v1.rknn` | 存在 |

### 缺失或未确认

| 组件 | 状态 |
| --- | --- |
| `rknn_toolkit_lite2` 命令 | 未找到 |
| Python RKNN Toolkit Lite2 包 | 未进一步确认 |

结论：板子侧 RKNN runtime 已经具备，适合运行已经转换好的 `.rknn` 模型。模型转换一般仍建议在 PC 或专门转换环境中完成，再把 `.rknn` 文件放到板子运行。

## 10. RGA、MPP 与硬件编解码设施

### RGA

已存在：

- `/dev/rga`
- `/usr/include/rga`
- `/lib/aarch64-linux-gnu/librga.so`
- `/usr/lib/aarch64-linux-gnu/pkgconfig/librga.pc`

RGA 可用于图像缩放、颜色空间转换、旋转、裁剪等硬件加速操作。对 AI 网关很有价值，尤其是摄像头图像预处理阶段。

### MPP / VPU

已存在：

- `/dev/mpp_service`
- `/usr/include/rockchip/*`
- `/lib/aarch64-linux-gnu/librockchip_mpp.so`
- `/lib/aarch64-linux-gnu/librockchip_vpu.so`
- `/usr/bin/mpp_info_test`
- `rockchip-mpp-demos`

MPP/VPU 可用于硬件视频编码和解码。对于 RTSP 拉流、视频转码、摄像头编码推流等场景，可以优先使用 MPP/GStreamer，降低 CPU 占用。

## 11. 外设与总线设施

已发现的外设节点：

| 类型 | 设备节点 |
| --- | --- |
| GPIO | `/dev/gpiochip0` 到 `/dev/gpiochip4` |
| I2C | `/dev/i2c-0` 到 `/dev/i2c-6` |
| UART | `/dev/ttyS0`, `/dev/ttyS3`, `/dev/ttyS4`, `/dev/ttyS7`, `/dev/ttyS8` |
| SPI | 未发现 `/dev/spidev*` |
| CAN | 未发现 `/dev/can*` |

系统工具方面：

- `gpiodetect` 存在
- `gpioinfo` 存在
- `i2cdetect` 存在
- `candump` / `cansend` 存在

结论：GPIO 和 I2C 设施已经暴露，UART 也有多个节点；SPI 和 CAN 工具存在或部分存在，但当前没有看到对应设备节点。若项目需要 SPI/CAN，可能需要检查设备树 overlay、内核配置或引脚复用。

## 12. USB 与 PCI

USB 控制器已识别多个 root hub：

- USB 2.0 root hub
- USB 3.0 root hub
- USB 1.1 root hub

当前未看到外接 USB 设备。`lspci` 没有输出，符合多数 RK3568 开发板无 PCIe 外设或未启用的情况。

## 13. 已安装开发工具链

### 已安装

| 工具 | 状态 |
| --- | --- |
| `gcc` | 已安装，版本 13.3.0 |
| `g++` | 已安装，版本 13.3.0 |
| `make` | 已安装 |
| `cmake` | 已安装 |
| `pkg-config` | 已安装 |
| `python3` | 已安装，版本 3.12.3 |
| `python3-pip` | 已安装 |
| `git` | 已安装 |
| `ffmpeg` | 已安装 |
| `gst-launch-1.0` | 已安装 |
| `v4l2-ctl` | 已安装 |
| `gpiodetect` / `gpioinfo` | 已安装 |
| `i2cdetect` | 已安装 |
| `candump` / `cansend` | 已安装 |

### 未安装

| 工具 | 状态 |
| --- | --- |
| `rustc` | 已安装，版本 1.98.1 |
| `cargo` | 已安装，版本 1.98.1 |
| `rustup` | 已安装，版本 1.29.1 |
| `node` | 已安装，版本 22.23.2 |
| `npm` | 已安装，版本 10.9.8 |
| `npx` | 已安装 |
| `docker` | 未安装 |
| `adb` | 未安装 |

结论：设备已经具备本地编译 Rust 后端、运行 Node 前端工具链和调试 C/C++/Python 组件的基础条件。对于大型 Rust 项目，仍建议优先在 PC 交叉编译，开发板保留为部署验证和必要时的本地构建环境。

## 14. 系统服务状态

### 正在运行的关键服务

| 服务 | 说明 |
| --- | --- |
| `ssh.service` | SSH 远程登录 |
| `NetworkManager.service` | 网络管理 |
| `dhcpcd.service` | 已停用，避免 Wi-Fi 获取重复 IPv4 地址 |
| `wpa_supplicant.service` | Wi-Fi 连接 |
| `lightdm.service` | 图形登录管理 |
| `bluetooth.service` | 蓝牙 |
| `vsftpd.service` | FTP |
| `ntpsec.service` | 时间同步 |
| `avahi-daemon.service` | mDNS |
| `cups.service` | 打印服务 |
| `kickpi-monitor.service` | KickPi 厂商监控服务 |
| `kickpi-fix-monitor.service` | KickPi 厂商修复监控服务 |

### 失败服务

| 服务 | 状态 | 原因摘要 | 影响判断 |
| --- | --- | --- | --- |
| `apport.service` | failed | 无法判断系统包管理器 | 对业务影响小 |
| `networking.service` | failed | `eth1` 拉起失败 | 当前 Wi-Fi 正常，短期影响小 |
| `rockchip.service` | failed | `/etc/init.d/rockchip.sh` 调用 `hwclock`，但命令不存在 | 可能影响部分 Rockchip 初始化逻辑 |

### systemd 配置警告

日志中还看到：

- `kickpi-boot.service` 等 service 文件被标记为可执行
- `rkaiq_3A.service` 被标记为 world-writable

这些不会必然导致系统不可用，但从系统整洁性和安全性来看，后续可以修正权限。

## 15. 温度与运行状态

检查时温度：

| 区域 | 温度 |
| --- | ---: |
| SoC | 约 50.6°C |
| GPU | 约 45.6°C |

这个温度在开发板空闲或轻负载状态下属于正常范围。后续进行多路视频、NPU 推理或长时间编译时，应继续观察温度、频率和是否降频。

## 16. 当前目录与用户数据

用户目录：

- `/root`
- `/home/kickpi`

未发现明显的用户项目目录，例如 gateway、rust、demo、rk 项目目录等。

其他观察：

- `/usr/local/test.mp4` 存在，大小约 8.5 MB
- `/opt` 基本为空
- `/userdata` 很小且当前不适合放置大数据

建议后续将网关程序按 Linux 服务化方式放置，例如：

```text
/opt/aigateway/
  bin/
  config/
  models/
  scripts/

/var/lib/aigateway/
  data/

/var/log/aigateway/
  logs
```

## 17. 对 GatewayRk3566Rust 项目的适配判断

当前项目名为 `GatewayRk3566Rust`，目标板实际为 RK3568。RK3566 和 RK3568 同属 Rockchip RK356x 系列，软件栈有较多共通处，但仍建议在文档和配置中明确目标设备为 RK3568/KickPi K1A，避免后续论文、部署脚本或硬件能力描述混乱。

### 推荐部署路线

优先推荐：

1. 在本机为 `aarch64-unknown-linux-gnu` 或合适的 musl/glibc 目标交叉编译 Rust 程序。
2. 将编译产物上传到开发板。
3. 在开发板上通过配置文件运行验证。
4. 稳定后再写成 `systemd` 服务。

不优先推荐：

- 直接在开发板上安装完整 Rust 工具链并本地编译。

原因：

- 开发板当前未安装 `rustc` / `cargo`
- 本地编译 Rust 项目会占用较多 CPU、内存和存储 IO
- 交叉编译更利于复现实验环境和自动化部署

## 18. 后续建议

### 短期建议

- 明确开发板固定 IP，解决 `wlan0` 双 IPv4 地址问题。
- 为 root 配置 SSH key，降低密码登录依赖。
- 如果不需要 FTP，关闭 `vsftpd.service`。
- 如果不需要 ADB 网络调试，关闭 `adbd` 或限制访问。
- 确认 `rockchip.service` 失败是否影响摄像头、NPU 或板级初始化。
- 确认 `/dev/video*` 是否对应真实接入摄像头，并测试采集链路。

### 中期建议

- 建立项目部署目录，例如 `/opt/aigateway`。
- 增加 `systemd` 服务文件，统一管理网关程序。
- 规划日志目录和日志轮转。
- 固化模型目录和配置文件路径。
- 建立一键部署脚本：交叉编译、上传、重启服务、查看状态。

### 长期建议

- 为论文实验记录固定系统版本、内核版本、模型版本和程序版本。
- 将 RKNN、RGA、MPP、摄像头链路的测试命令整理成实验附录。
- 若项目涉及长期运行，增加看门狗、健康检查和异常恢复策略。
- 若部署到非隔离网络，做端口收敛和账号加固。

## 19. 总结

这台 RK3568 开发板当前具备比较完整的 AI 网关开发基础：

- Ubuntu 24.04 arm64 系统可用
- CPU、内存、存储资源充足
- Wi-Fi 网络可用，SSH 已可连接
- 摄像头 ISP、V4L2、RKAIQ 设施存在
- RGA、MPP/VPU、RKNN runtime 已安装
- GPIO、I2C、UART 等板级外设节点已暴露
- C/C++、Python、多媒体和外设调试工具较完整

主要短板和风险是：

- 触摸屏目前内核层没有收到触摸中断，详见触摸专项排查
- FTP、ADB 5555 等服务开放较多
- `rockchip.service`、`networking.service` 等服务存在失败项
- root 密码较弱，不适合暴露在不可信网络中

因此，该设备适合作为 `GatewayRk3566Rust` 项目的真实 RK3568 部署与验证平台。下一步建议围绕“交叉编译、上传运行、服务化部署、摄像头/NPU链路验证”继续推进。

## 20. 2026-09-08 部署前调整记录

### 已完成配置

| 项目 | 状态 |
| --- | --- |
| Swap | 已创建并启用 4 GiB `/swapfile`，已写入 `/etc/fstab` |
| Wi-Fi 重复地址 | 已停用 `dhcpcd`，避免与 NetworkManager 同时 DHCP |
| Rust 后端环境 | 已安装 `/opt/rust`，全局可用 `rustc`、`cargo`、`rustup` |
| 前端环境 | 已安装 `/opt/node-v22`，全局可用 `node`、`npm`、`npx` |
| 基础构建工具 | 已补齐 `git`、`gcc/g++`、`make`、`cmake`、`pkg-config`、`libssl-dev` 等 |
| 屏幕熄灭策略 | 已设置 10 分钟空闲 blank，关闭 DPMS，并将 XFCE 空闲整机休眠明确设为禁用 |
| 横屏触摸矩阵 | 已增加 X11 登录钩子，根据 `DSI-1` 旋转状态设置 Goodix 触摸坐标矩阵 |

### 屏幕与触摸专项排查

当前显示状态：

```text
DSI-1 connected 1280x800+0+0 left
```

Goodix 触摸设备在内核和 X11 中都能注册：

```text
input: goodix-ts as /devices/virtual/input/input2
/dev/input/event2
```

当前 XInput 横屏矩阵为：

```text
0 -1 1
1  0 0
0  0 1
```

这个矩阵对应 `DSI-1 left` 横屏方向，说明 X11 坐标旋转配置本身已经生效。

进一步做过两组关键验证：

1. 切回竖屏 `normal` 并使用 identity 触摸矩阵后，`evtest /dev/input/event2` 仍然没有收到任何触摸事件。
2. 临时停止 `input-event-daemon` 后，`evtest /dev/input/event2` 仍然没有收到任何触摸事件，且 `/proc/interrupts` 中 `gt9xx` 中断计数前后不变。

因此当前触摸失效不是普通的 X11 坐标旋转问题，也不是输入守护进程抢占事件导致。更接近以下低层原因：

- 触摸排线、触摸模组或触摸供电异常。
- Goodix INT 中断线没有实际跳变，或设备树中的 IRQ GPIO/触发方式与硬件不匹配。
- 当前屏幕/触摸模组与镜像内置的 Goodix 配置不完全匹配。
- Goodix 控制器处于异常休眠或固件配置状态，虽然 I2C 设备能注册，但触摸扫描没有产生上报。

当前不建议继续用软件热重绑 Goodix 驱动来测试。此前重绑会触发 I2C 通信失败，容易让触摸设备临时消失，必须重启才能恢复到注册状态。

### 建议下一步

优先检查硬件侧：

- 重新插拔并压紧触摸 FPC，不只检查显示排线。
- 确认屏幕模组的触摸芯片型号和当前镜像设备树中的 `goodix,gt9xx` 是否一致。
- 若有厂家原始横屏镜像或设备树，重点比对 Goodix 的 `irq-gpio`、`reset-gpio`、`gtp_resolution_x/y`、`gtp_change_x2y`、`gtp_overturn_x/y`、`gtp_int_tarigger`。

软件侧建议：

- 保留当前“10 分钟 blank，禁用 DPMS”的屏幕策略。
- 保留当前横屏 XInput 矩阵脚本。
- 不再读取 `/proc/gt9xx_config`，此前该接口在当前驱动上触发过内核告警。

## 21. KICKPI K1A 屏幕与触摸深度分析

### 板卡、屏幕和触摸控制器识别

| 层级 | 实际识别结果 | 说明 |
| --- | --- | --- |
| 厂家/系列 | KICKPI K1A | 设备树型号为 `Rockchip RK3568 KICKPI K1A Board` |
| SoC | Rockchip RK3568，四核 Cortex-A55 | 厂家 K1 资料对应 RK3568B2 平台 |
| PCB/底板配置 | `rockchip,rk3568-kickpi-k1a` | 这是当前启动镜像实际加载的板级配置，不是通用 RK3568 配置 |
| 显示接口 | MIPI DSI0，4 Lane | DRM 节点为 `DSI-1`，原生模式 `800x1280` |
| 显示控制器 | JD9365 系列配置特征 | 根据面板初始化序列判断；系统设备树没有写入精确屏幕商品型号 |
| 触摸控制器 | Goodix GT911 | 驱动启动日志读出 `IC Version: 911_1060`、`Sensor_ID: 0` |
| 触摸总线 | I2C1，地址 `0x5d` | 启动时通信成功 |
| 触摸驱动 | 厂家 GT9XX V2.4，日期 2014-11-28 | 内置于厂家 Linux 6.1.141 内核，不是主线 Goodix 驱动 |

KICKPI 官方资料列出的 800x1280 MIPI 屏不止一种，包括 10.1 寸 `AT101DS40I`、10.1 寸 `MX101BA1340` 和 8 寸 `MX080B2140`。它们分属不同的设备树配置。当前镜像没有 `panel-name`，显示驱动和 Goodix 驱动启动时都报告缺少该属性，因此系统无法仅靠软件确认接入的是哪一个商品型号。

### 当前设备树中的触摸接线

| 信号 | RK3568 GPIO | 当前状态 |
| --- | --- | --- |
| GT911 RESET | GPIO0_B6，Linux 全局编号 14 | 高电平 |
| GT911 INT | GPIO3_A3，Linux 全局编号 99 | 高电平，IRQ 93 |
| MIPI/触摸模组 3.3V | `vcc3v3_mipi_con` | 已启用，3.3V |
| 面板 RESET | GPIO3_A4 | 与触摸 INT 相邻但不是同一引脚 |

Goodix 配置块长度为 186 字节，配置版本为 `0x70`，其中分辨率确实是 X=800、Y=1280，配置校验字节也存在。驱动启动时完成硬复位、读到 GT911 型号、读到 Sensor ID，并成功下发配置。因此 I2C 地址、基础供电和 RESET 在启动阶段至少是可工作的。

### 已确认的两个问题

#### 1. 熄屏后触摸不能唤醒

这不是单纯的屏幕 DPMS 问题。上一轮启动日志在开机约 608 秒时明确结束于：

```text
PM: suspend entry (deep)
```

也就是系统在约 10 分钟后进入了整机深度休眠。与此同时，GT911 对应的 I2C 和 input 设备都没有 `power/wakeup` 节点，设备树也没有把它声明成可靠的系统唤醒源。因此整机休眠后，点击触摸屏不能唤醒是当前驱动/设备树下的必然结果。

已把 XFCE 的 `inactivity-on-ac` 和 `inactivity-on-battery` 都设为 0，禁止空闲后整机休眠；保留 10 分钟黑屏，DPMS 继续禁用。这样设备只显示黑屏，不应再自动进入深度休眠。

#### 2. 重启后触摸仍完全无事件

这一问题发生在 X11 坐标转换之前，证据如下：

- `evtest /dev/input/event2` 在竖屏和横屏下都收不到事件。
- 点击期间 IRQ 93 的 `gt9xx` 计数始终为 3，没有增加。
- GPIO3_A3（GT911 INT）点击期间始终为高电平。
- 停止 `input-event-daemon` 后结果不变，排除用户态程序抢占。
- GT911 在内核启动约 3.3 秒时能够通信和下发配置，但进入桌面后对 `0x814e` 状态寄存器的安全读取不再应答。

因此当前失效点位于 Goodix 控制器或其硬件/内核管理链路，不在 XInput 旋转矩阵。结合“旋转后开始失效”的时间关系，优先怀疑厂家 2014 年 GT9XX 驱动在 Linux 6.1 DRM 模式切换、面板 blank/unblank 或 suspend 回调中的兼容问题，使 GT911 进入睡眠后没有正确唤醒。次要可能是触摸 FPC 的 INT 线接触不良、模组供电不稳，或刷入的 800x1280 屏幕配置与实际面板型号不一致。

### 原因概率排序

1. **高概率：厂家旧 GT9XX 驱动的休眠/显示模式切换兼容问题。** 启动时 GT911 正常，进入桌面后不应答；故障又与旋转和熄屏操作有明确时间关联。
2. **中高概率：触摸模组或 FPC 的 INT/供电链路问题。** I2C 和 RESET 曾经工作，但 INT 始终不跳，单根触点接触不良仍然可能。
3. **中概率：镜像选错具体屏幕变体。** 当前缺少 `panel-name`，而 KICKPI 对多个 800x1280 模组使用不同配置；错误的 Sensor 配置可导致 GT911 扫描异常。
4. **低概率：X11 横屏矩阵或桌面输入服务。** 已经通过竖屏、直接 `evtest` 和停止输入守护进程排除。

### 建议处理顺序

1. 完全关机并拔掉 12V 电源 10 秒以上后再上电，而不是只执行软重启。这样才能让常供电的 `vcc3v3_mipi_con` 和 GT911 真正掉电复位。
2. 上电后先不要做旋转或熄屏操作，立即测试触摸；随后再应用横屏。这个对照能确认是否由 DRM/GT9XX 的 blank 回调触发。
3. 重新压紧或更换触摸 FPC，重点检查 GT911 INT 对应触点；显示画面正常并不能证明触摸排线完全正常。
4. 从屏幕背面标签确认精确型号是 `AT101DS40I`、`MX101BA1340`、`MX080B2140` 或其他型号，再选择厂家对应镜像/设备树。
5. 若冷启动后触摸正常、应用横屏后立即失效，应修补或更换 GT9XX 内核驱动的 suspend/resume/FB notifier 逻辑，而不是继续调整 XInput 矩阵。
