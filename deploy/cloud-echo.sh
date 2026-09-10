#!/usr/bin/env bash
# 云侧验收脚本：订阅全部网关主题（在云服务器或任意可达 MQTT broker 的机器上运行）
# 依赖：mosquitto-clients（apt install mosquitto-clients）
set -euo pipefail
HOST="${1:-115.159.222.238}"
mosquitto_sub -h "$HOST" -p 1883 -t 'gateway/#' -v
