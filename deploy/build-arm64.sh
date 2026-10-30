#!/usr/bin/env bash
# RK3568 交叉编译脚本：产出 aarch64 Linux 二进制（Ubuntu 22.04 glibc 可直接运行）
# 前置：cargo install cross / rustup target add aarch64-unknown-linux-gnu
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET="${TARGET:-aarch64-unknown-linux-gnu}"
MODE="${MODE:-release}"

case "$MODE" in
    debug)
        CARGO_PROFILE_ARGS=()
        OUTPUT_DIR="debug"
        ;;
    release)
        CARGO_PROFILE_ARGS=(--release)
        OUTPUT_DIR="release"
        ;;
    dist)
        CARGO_PROFILE_ARGS=(--profile dist)
        OUTPUT_DIR="dist"
        ;;
    *)
        echo "不支持的 MODE=$MODE（可选：debug、release、dist）" >&2
        exit 2
        ;;
esac

if command -v npm >/dev/null 2>&1; then
    npm --prefix web-ui ci
    npm --prefix web-ui run build
else
    echo "未找到 npm，无法构建 Web UI" >&2
    exit 1
fi

if command -v cross >/dev/null 2>&1; then
    cross build --target "$TARGET" "${CARGO_PROFILE_ARGS[@]}" -p gw-web -p gw-agent -p gw-ui
else
    rustup target add "$TARGET"
    # 需要系统已装 aarch64-linux-gnu- 工具链，并在 .cargo/config.toml 配置 linker
    cargo build --target "$TARGET" "${CARGO_PROFILE_ARGS[@]}" -p gw-web -p gw-agent -p gw-ui
fi

echo "产物："
ls -lh target/"$TARGET"/"$OUTPUT_DIR"/gateway-web target/"$TARGET"/"$OUTPUT_DIR"/gateway-headless target/"$TARGET"/"$OUTPUT_DIR"/park-gateway
