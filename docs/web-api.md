# Gateway Web API

`gateway-web` 在同一进程内运行采集 Agent、HTTP/WebSocket API，并托管 `web-ui/dist` Vue 生产构建。默认监听 `0.0.0.0:8080`。

## 启动

```powershell
npm --prefix web-ui run build
cargo run -p gw-web --bin gateway-web -- --config config/gateway.toml --web-root web-ui/dist
```

RK3566 生产启动：

```sh
gateway-web --config /etc/park-gateway/gateway.toml --web-root /usr/share/park-gateway/web-ui
```

## 接口

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/api/v1/health` | 进程与云链路健康检查 |
| GET | `/api/v1/snapshot` | Vue 首屏完整快照 |
| GET | `/api/v1/ws` | 快照 WebSocket 实时推送 |
| GET | `/api/v1/events?limit=100` | 运行/告警日志 |
| GET | `/api/v1/commands?limit=100` | 平台下行指令日志 |
| GET | `/api/v1/uploads?limit=100` | Outbox 待发上报列表 |
| GET/POST | `/api/v1/meters` | 设备列表/新建设备 |
| PUT/DELETE | `/api/v1/meters/{id}` | 修改/删除设备 |
| POST | `/api/v1/meters/{id}/read` | 立即采集并进入上报链路 |
| GET | `/api/v1/network` | Wi-Fi 当前状态 |
| GET/POST | `/api/v1/network/scan` | 主动扫描 Wi-Fi |
| POST | `/api/v1/network/connect` | 连接 Wi-Fi |
| POST | `/api/v1/network/disconnect` | 断开 Wi-Fi |
| POST | `/api/v1/network/forget` | 忘记 Wi-Fi |
| GET/POST | `/api/v1/platform` | 读取/保存网关与 MQTT 配置 |
| POST | `/api/v1/platform/diagnose` | 云链路诊断 |

HTTP 错误统一返回 `{ "code": "...", "message": "..." }`；密码不会从配置读取接口返回。
