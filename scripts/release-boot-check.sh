#!/usr/bin/env bash
# release-boot-check.sh — Contract #9 boot-log assertion guardrail.
# Added in 0.38.3 to catch the 0.38.2-shaped regression where senko serve
# silently disabled OTel exporters. Pinned schema (traces.status / logs.status
# string literals "enabled"/"disabled", SENKO_LOG_FORMAT=json) is asserted
# with jq.
set -euo pipefail

cd "$(dirname "$0")/.."

command -v jq >/dev/null || { echo "FAIL: jq is required" >&2; exit 1; }

cargo build -q --bin senko

LOG=$(mktemp)
trap 'rm -f "$LOG"' EXIT

OTEL_LOGS_EXPORTER=otlp \
OTEL_TRACES_EXPORTER=otlp \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:65535 \
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
OTEL_BSP_SCHEDULE_DELAY=200 \
OTEL_BLRP_SCHEDULE_DELAY=200 \
SENKO_LOG_FORMAT=json \
SENKO_AUTH_API_KEY_MASTER_KEY=release-boot-check \
./target/debug/senko serve --port 0 >"$LOG" 2>&1 &
SENKO_PID=$!
# GNU `timeout` is unavailable on stock macOS — emulate `timeout 3s`.
sleep 3
kill "$SENKO_PID" 2>/dev/null || true
wait "$SENKO_PID" 2>/dev/null || true

echo "=== senko serve boot log ==="
cat "$LOG"
echo "============================"

INIT_LINE=$(grep -F '"OTel telemetry initialized"' "$LOG" | head -1 || true)

if [[ -z "$INIT_LINE" ]]; then
  if grep -q 'without exporters' "$LOG"; then
    echo "FAIL: boot log shows 'without exporters' — OTel exporters were silently disabled" >&2
    echo "      check Cargo.toml opentelemetry-otlp features (reqwest-blocking-client) and src/bootstrap.rs init_telemetry" >&2
  else
    echo "FAIL: boot log missing 'OTel telemetry initialized'" >&2
  fi
  exit 1
fi

assert_field() {
  local key="$1" want="$2"
  local got
  got=$(echo "$INIT_LINE" | jq -r ".fields.\"${key}\"")
  if [[ "$got" != "$want" ]]; then
    echo "FAIL: ${key} expected \"${want}\", got \"${got}\"" >&2
    exit 1
  fi
}

assert_field "traces.status"   "enabled"
assert_field "logs.status"     "enabled"
assert_field "traces.protocol" "http/protobuf"
assert_field "logs.protocol"   "http/protobuf"

echo "OK: OTel boot log assertion passed"
