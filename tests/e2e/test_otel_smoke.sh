#!/usr/bin/env bash
# E2E smoke test: senko serve actually exports LogRecords and Spans to an
# OTLP collector for both grpc and http/protobuf, and the boot log matches
# the schema pinned by Contract #9. A 0.38.2-shaped silent disablement
# (Cargo feature gap) must turn this test red.
source "$(dirname "$0")/helpers.sh"

setup_test_env

MOCK_BIN="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/target/debug/mock-otel-collector"
if [[ ! -x "$MOCK_BIN" ]]; then
  echo "FATAL: mock-otel-collector not built at $MOCK_BIN" >&2
  echo "       run: cargo build -p mock-otel-collector" >&2
  exit 1
fi

MOCK_PID=""
SENKO_PID=""
cleanup_all() {
  [[ -n "$SENKO_PID" ]] && kill "$SENKO_PID" 2>/dev/null || true
  [[ -n "$MOCK_PID"  ]] && kill "$MOCK_PID"  2>/dev/null || true
  cleanup_test_env
}
trap cleanup_all EXIT

run_otel_case() {
  local protocol="$1"     # grpc | http/protobuf
  local case_label="$2"   # short label used in test messages and log filenames
  local senko_port
  senko_port=$(allocate_port)

  local mock_log="$TEST_DIR/mock-${case_label}.log"
  local senko_log="$TEST_DIR/senko-${case_label}.log"

  echo "=== otel smoke case: protocol=${protocol} ==="

  "$MOCK_BIN" >"$mock_log" 2>&1 &
  MOCK_PID=$!

  wait_for "mock-collector announces both ports (${case_label})" 10 \
    "grep -q 'listen-port-http=' '$mock_log' && grep -q 'listen-port-grpc=' '$mock_log'"

  local http_port grpc_port endpoint_port
  http_port=$(grep -oE 'listen-port-http=[0-9]+' "$mock_log" | head -1 | cut -d= -f2)
  grpc_port=$(grep -oE 'listen-port-grpc=[0-9]+' "$mock_log" | head -1 | cut -d= -f2)
  if [[ "$protocol" == "grpc" ]]; then
    endpoint_port="$grpc_port"
  else
    endpoint_port="$http_port"
  fi

  # SENKO_LOG_FORMAT=json + Contract #9 jq assertions, plus tight batch flush
  # so we don't depend on the SDK's default 5s schedule.
  OTEL_LOGS_EXPORTER=otlp \
  OTEL_TRACES_EXPORTER=otlp \
  OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:${endpoint_port}" \
  OTEL_EXPORTER_OTLP_PROTOCOL="${protocol}" \
  OTEL_BSP_SCHEDULE_DELAY=200 \
  OTEL_BLRP_SCHEDULE_DELAY=200 \
  SENKO_LOG_FORMAT=json \
  SENKO_AUTH_API_KEY_MASTER_KEY="otel-smoke-${case_label}" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" serve --port "$senko_port" \
    >"$senko_log" 2>&1 &
  SENKO_PID=$!

  wait_for "senko emits boot log (${case_label})" 15 \
    "grep -q 'OTel telemetry initialized' '$senko_log'"

  local boot_line
  boot_line=$(grep '"OTel telemetry initialized"' "$senko_log" | head -1)
  assert_eq "enabled"    "$(echo "$boot_line" | jq -r '.fields["traces.status"]')"   "${case_label}: traces.status=enabled"
  assert_eq "enabled"    "$(echo "$boot_line" | jq -r '.fields["logs.status"]')"     "${case_label}: logs.status=enabled"
  assert_eq "$protocol"  "$(echo "$boot_line" | jq -r '.fields["traces.protocol"]')" "${case_label}: traces.protocol=${protocol}"
  assert_eq "$protocol"  "$(echo "$boot_line" | jq -r '.fields["logs.protocol"]')"   "${case_label}: logs.protocol=${protocol}"
  assert_not_contains    "$(cat "$senko_log")" "without exporters"                   "${case_label}: no 'without exporters' in log"

  wait_for "senko HTTP is ready (${case_label})" 10 \
    "curl -sf http://127.0.0.1:${senko_port}/api/v1/health >/dev/null"

  # One real HTTP request -> guarantees a request span enters the pipeline.
  curl -sf "http://127.0.0.1:${senko_port}/api/v1/health" >/dev/null

  local received logs_n=0 spans_n=0 ok=0
  for _ in $(seq 1 30); do
    received=$(curl -sf "http://127.0.0.1:${http_port}/__received" 2>/dev/null || echo '{}')
    logs_n=$(echo "$received" | jq -r '.logs // 0')
    spans_n=$(echo "$received" | jq -r '.spans // 0')
    if [[ "$logs_n" -gt 0 && "$spans_n" -gt 0 ]]; then
      ok=1
      break
    fi
    sleep 0.5
  done
  assert_eq "1" "$ok" \
    "${case_label}: collector received ≥1 log batch (got ${logs_n}) and ≥1 span batch (got ${spans_n})"

  kill "$SENKO_PID" 2>/dev/null || true; wait "$SENKO_PID" 2>/dev/null || true; SENKO_PID=""
  kill "$MOCK_PID"  2>/dev/null || true; wait "$MOCK_PID"  2>/dev/null || true; MOCK_PID=""
}

run_otel_case grpc           grpc
run_otel_case http/protobuf  httpproto

test_summary
