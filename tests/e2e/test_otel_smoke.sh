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
UPSTREAM_PID=""
cleanup_all() {
  [[ -n "$SENKO_PID"    ]] && kill "$SENKO_PID"    2>/dev/null || true
  [[ -n "$UPSTREAM_PID" ]] && kill "$UPSTREAM_PID" 2>/dev/null || true
  [[ -n "$MOCK_PID"     ]] && kill "$MOCK_PID"     2>/dev/null || true
  cleanup_test_env
}
trap cleanup_all EXIT

# Wait until the mock collector reports a non-null service.name for both
# signals on /__received, then return its values via two named globals.
# Sets `OBSERVED_TRACES_SERVICE_NAME` and `OBSERVED_LOGS_SERVICE_NAME`.
wait_for_service_names() {
  local http_port="$1"
  local case_label="$2"
  local received traces_name='' logs_name=''
  for _ in $(seq 1 30); do
    received=$(curl -sf "http://127.0.0.1:${http_port}/__received" 2>/dev/null || echo '{}')
    traces_name=$(echo "$received" | jq -r '.traces_service_name // empty')
    logs_name=$(echo "$received" | jq -r '.logs_service_name // empty')
    if [[ -n "$traces_name" && -n "$logs_name" ]]; then
      OBSERVED_TRACES_SERVICE_NAME="$traces_name"
      OBSERVED_LOGS_SERVICE_NAME="$logs_name"
      return 0
    fi
    sleep 0.5
  done
  echo "FAIL: ${case_label}: collector did not report service.name (traces='${traces_name}' logs='${logs_name}')" >&2
  OBSERVED_TRACES_SERVICE_NAME="$traces_name"
  OBSERVED_LOGS_SERVICE_NAME="$logs_name"
  return 1
}

run_otel_case() {
  local protocol="$1"     # grpc | http/protobuf
  local case_label="$2"   # short label used in test messages and log filenames
  local extra_env_var="${3:-}"   # optional "NAME=value" passed as an extra senko env var
  local expected_service_name="${4:-senko-server}"  # expected service.name in resource attrs
  local senko_port
  senko_port=$(allocate_port)
  # Bash treats a literal `KEY=value` token before a command as an env var
  # assignment, but a `${var}` expansion that *contains* `KEY=value` is parsed
  # as a regular argument — so we route optional env vars through `env`.
  local extra_prefix=()
  if [[ -n "$extra_env_var" ]]; then
    extra_prefix=(env "$extra_env_var")
  fi

  local mock_log="$TEST_DIR/mock-${case_label}.log"
  local senko_log="$TEST_DIR/senko-${case_label}.log"

  echo "=== otel smoke case: protocol=${protocol}${extra_env_var:+ ${extra_env_var}} ==="

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
  "${extra_prefix[@]}" \
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

  wait_for_service_names "$http_port" "$case_label" || true
  assert_eq "$expected_service_name" "$OBSERVED_TRACES_SERVICE_NAME" \
    "${case_label}: traces resource service.name=${expected_service_name}"
  assert_eq "$expected_service_name" "$OBSERVED_LOGS_SERVICE_NAME" \
    "${case_label}: logs resource service.name=${expected_service_name}"

  kill "$SENKO_PID" 2>/dev/null || true; wait "$SENKO_PID" 2>/dev/null || true; SENKO_PID=""
  kill "$MOCK_PID"  2>/dev/null || true; wait "$MOCK_PID"  2>/dev/null || true; MOCK_PID=""
}

# Spin up an upstream `senko serve` (no OTLP) plus a relay (`SENKO_SERVER_RELAY_URL`
# pointing at the upstream). The relay is the one whose telemetry we observe — it
# must default to `service.name=senko-relay`. We send traffic against the relay's
# HTTP port so the relay's own tracer pipeline emits at least one span/log batch.
run_relay_default_case() {
  local case_label="relay-default"
  local upstream_port relay_port
  upstream_port=$(allocate_port 0)
  relay_port=$(allocate_port 1)

  local mock_log="$TEST_DIR/mock-${case_label}.log"
  local upstream_log="$TEST_DIR/upstream-${case_label}.log"
  local relay_log="$TEST_DIR/senko-${case_label}.log"

  echo "=== otel smoke case: relay-mode default service.name ==="

  "$MOCK_BIN" >"$mock_log" 2>&1 &
  MOCK_PID=$!

  wait_for "mock-collector announces both ports (${case_label})" 10 \
    "grep -q 'listen-port-http=' '$mock_log' && grep -q 'listen-port-grpc=' '$mock_log'"

  local http_port grpc_port
  http_port=$(grep -oE 'listen-port-http=[0-9]+' "$mock_log" | head -1 | cut -d= -f2)
  grpc_port=$(grep -oE 'listen-port-grpc=[0-9]+' "$mock_log" | head -1 | cut -d= -f2)

  # Upstream: telemetry off so its boot log doesn't race against the relay's
  # exports for the mock collector's "first batch".
  OTEL_SDK_DISABLED=true \
  SENKO_AUTH_API_KEY_MASTER_KEY="otel-smoke-${case_label}-upstream" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" serve --port "$upstream_port" \
    >"$upstream_log" 2>&1 &
  UPSTREAM_PID=$!

  wait_for "upstream HTTP is ready (${case_label})" 10 \
    "curl -sf http://127.0.0.1:${upstream_port}/api/v1/health >/dev/null"

  # Relay: OTLP exporter pointed at mock collector. The relay's
  # `init_telemetry` is called with `TelemetryMode::Relay`, so we expect
  # service.name=senko-relay.
  OTEL_LOGS_EXPORTER=otlp \
  OTEL_TRACES_EXPORTER=otlp \
  OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:${grpc_port}" \
  OTEL_EXPORTER_OTLP_PROTOCOL="grpc" \
  OTEL_BSP_SCHEDULE_DELAY=200 \
  OTEL_BLRP_SCHEDULE_DELAY=200 \
  SENKO_LOG_FORMAT=json \
  SENKO_SERVER_RELAY_URL="http://127.0.0.1:${upstream_port}" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" serve --port "$relay_port" \
    >"$relay_log" 2>&1 &
  SENKO_PID=$!

  wait_for "relay emits boot log (${case_label})" 15 \
    "grep -q 'OTel telemetry initialized' '$relay_log'"

  wait_for "relay HTTP is ready (${case_label})" 10 \
    "curl -sf http://127.0.0.1:${relay_port}/api/v1/health >/dev/null"

  curl -sf "http://127.0.0.1:${relay_port}/api/v1/health" >/dev/null

  wait_for_service_names "$http_port" "$case_label" || true
  assert_eq "senko-relay" "$OBSERVED_TRACES_SERVICE_NAME" \
    "${case_label}: traces resource service.name=senko-relay"
  assert_eq "senko-relay" "$OBSERVED_LOGS_SERVICE_NAME" \
    "${case_label}: logs resource service.name=senko-relay"

  kill "$SENKO_PID"    2>/dev/null || true; wait "$SENKO_PID"    2>/dev/null || true; SENKO_PID=""
  kill "$UPSTREAM_PID" 2>/dev/null || true; wait "$UPSTREAM_PID" 2>/dev/null || true; UPSTREAM_PID=""
  kill "$MOCK_PID"     2>/dev/null || true; wait "$MOCK_PID"     2>/dev/null || true; MOCK_PID=""
}

run_otel_case grpc           grpc
run_otel_case http/protobuf  httpproto
run_otel_case grpc           svcname-override "OTEL_SERVICE_NAME=senko-custom" "senko-custom"
run_relay_default_case

test_summary
