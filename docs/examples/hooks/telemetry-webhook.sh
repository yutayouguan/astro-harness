#!/usr/bin/env bash
# telemetry-webhook.sh — 将 Astro hook 事件以 JSON 格式 POST 到外部端点。
#
# 环境变量（由 Astro 自动注入）：
#   ASTRO_HOOK_EVENT    — canonical 事件名（如 PreGatewayDispatch）
#   ASTRO_HOOK_SESSION  — 会话 ID
#   ASTRO_HOOK_TURN     — turn_id（若有）
#   ASTRO_HOOK_TOOL     — 工具名（若有）
#   ASTRO_HOOK_DETAIL   — 事件摘要
#
# 用户需自行设置：
#   ASTRO_TELEMETRY_URL   — POST 目标 URL（如 https://your-endpoint/ingest）
#   ASTRO_TELEMETRY_TOKEN — 可选 Bearer Token（留空则不带 Authorization 头）
#   ASTRO_TELEMETRY_INCLUDE_DETAIL — 可选；设为 1 才发送可能含敏感内容的 detail
#
# 使用示例（~/.astro/config.yaml）：
#   hooks:
#     GatewayStartup:     '"$HOME/.astro/hooks/telemetry-webhook.sh"'
#     PreGatewayDispatch:  '"$HOME/.astro/hooks/telemetry-webhook.sh"'
#     CommandNewChat:      '"$HOME/.astro/hooks/telemetry-webhook.sh"'
#
# 此脚本发送 Astro 自定义 JSON；请使用自建 webhook，或自行适配后再转发至 Langfuse、OpenTelemetry 等后端。

set -euo pipefail

# 未配置目标时静默退出，不影响 agent 主循环。
if [[ -z "${ASTRO_TELEMETRY_URL:-}" ]]; then
    exit 0
fi

EVENT="${ASTRO_HOOK_EVENT:-}"
SESSION="${ASTRO_HOOK_SESSION:-}"
TURN="${ASTRO_HOOK_TURN:-}"
TOOL="${ASTRO_HOOK_TOOL:-}"
DETAIL=""
if [[ "${ASTRO_TELEMETRY_INCLUDE_DETAIL:-}" == "1" ]]; then
    DETAIL="${ASTRO_HOOK_DETAIL:-}"
fi
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"

# 构造 JSON payload（不含 tool_input/tool_response，避免敏感数据外泄）。
PAYLOAD="$(printf '{"event":%s,"session_id":%s,"turn_id":%s,"tool":%s,"detail":%s,"ts":%s}' \
    "$(printf '%s' "$EVENT"   | jq -Rs '.')" \
    "$(printf '%s' "$SESSION" | jq -Rs '.')" \
    "$(printf '%s' "$TURN"    | jq -Rs '.')" \
    "$(printf '%s' "$TOOL"    | jq -Rs '.')" \
    "$(printf '%s' "$DETAIL"  | jq -Rs '.')" \
    "$(printf '%s' "$TS"      | jq -Rs '.')")"

CURL_ARGS=(-sS -m 4 -X POST -H "Content-Type: application/json" -d "$PAYLOAD")

if [[ -n "${ASTRO_TELEMETRY_TOKEN:-}" ]]; then
    CURL_ARGS+=(-H "Authorization: Bearer ${ASTRO_TELEMETRY_TOKEN}")
fi

curl "${CURL_ARGS[@]}" "$ASTRO_TELEMETRY_URL" >/dev/null 2>&1 || true
