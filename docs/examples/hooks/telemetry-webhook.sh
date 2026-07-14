#!/usr/bin/env bash
# Astro Shell Hook telemetry webhook example.
# Copy to ~/.astro/hooks/telemetry-webhook.sh && chmod +x
# Set ASTRO_TELEMETRY_URL (optional ASTRO_TELEMETRY_TOKEN).

set -u
url="${ASTRO_TELEMETRY_URL:-}"
if [[ -z "$url" ]]; then
  exit 0
fi

# Minimal JSON string escape (no jq): backslash and double-quote only.
json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

ts="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u)"
event="${ASTRO_HOOK_EVENT:-}"
session="${ASTRO_HOOK_SESSION:-}"
turn="${ASTRO_HOOK_TURN:-}"
tool="$(json_escape "${ASTRO_HOOK_TOOL:-}")"
detail="$(json_escape "${ASTRO_HOOK_DETAIL:-}")"

json=$(printf '{"ts":"%s","event":"%s","session_id":"%s","turn_id":"%s","tool":"%s","detail":"%s"}' \
  "$ts" "$event" "$session" "$turn" "$tool" "$detail")

auth=()
if [[ -n "${ASTRO_TELEMETRY_TOKEN:-}" ]]; then
  auth=(-H "Authorization: Bearer ${ASTRO_TELEMETRY_TOKEN}")
fi

curl -sS -m 4 -X POST "$url" \
  -H "Content-Type: application/json" \
  "${auth[@]}" \
  -d "$json" >/dev/null 2>&1 || true

exit 0
