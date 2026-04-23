#!/usr/bin/env bash
# E2E test: Dependency management through relay chain (CLI → Relay → Upstream)
# Tests both passthrough-token mode and relay-token mode.

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

UPSTREAM_PID=""
RELAY_PID=""
MASTER_KEY=test-key

UPSTREAM_PORT=$(allocate_port 0)
RELAY_PORT=$(allocate_port 1)
UPSTREAM_URL="http://127.0.0.1:$UPSTREAM_PORT"
RELAY_URL="http://127.0.0.1:$RELAY_PORT"

start_upstream() {
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
    serve --port "$UPSTREAM_PORT" >/dev/null 2>&1 &
  UPSTREAM_PID=$!
  wait_for "upstream server ready" 10 "curl -sf $UPSTREAM_URL/api/v1/health >/dev/null"
}

# Start relay with optional SENKO_SERVER_RELAY_TOKEN
start_relay() {
  local relay_token="${1:-}"
  local env_args=()
  env_args+=(SENKO_SERVER_RELAY_URL="$UPSTREAM_URL")
  if [[ -n "$relay_token" ]]; then
    env_args+=(SENKO_SERVER_RELAY_TOKEN="$relay_token")
  fi

  env "${env_args[@]}" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    serve --port "$RELAY_PORT" >/dev/null 2>&1 &
  RELAY_PID=$!
  wait_for "relay server ready" 10 "curl -sf $RELAY_URL/api/v1/health >/dev/null"
}

stop_servers() {
  if [[ -n "$RELAY_PID" ]]; then
    kill "$RELAY_PID" 2>/dev/null || true
    wait "$RELAY_PID" 2>/dev/null || true
    RELAY_PID=""
  fi
  if [[ -n "$UPSTREAM_PID" ]]; then
    kill "$UPSTREAM_PID" 2>/dev/null || true
    wait "$UPSTREAM_PID" 2>/dev/null || true
    UPSTREAM_PID=""
  fi
}

cleanup_all() {
  stop_servers
  cleanup_test_env
}
trap cleanup_all EXIT

run_relay() {
  SENKO_CLI_REMOTE_URL="$RELAY_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

# Helper: direct curl POST through relay (bypasses CLI, 5s timeout)
relay_post() {
  local path="$1"
  local body="$2"
  curl -sf --max-time 5 \
    -H "Authorization: Bearer $TEST_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$body" \
    "$RELAY_URL$path"
}

# ========================================
# Part 1: Passthrough-token mode (no SENKO_SERVER_RELAY_TOKEN)
# ========================================
echo "--- Part 1: deps via relay (passthrough token) ---"

start_upstream
start_relay  # no relay token — uses PASSTHROUGH_TOKEN
TEST_TOKEN=$(create_test_user_key "$UPSTREAM_URL" "$MASTER_KEY")

A_ID="$(run_relay task add --title "Task A" | jq -r '.id')"
B_ID="$(run_relay task add --title "Task B" | jq -r '.id')"
C_ID="$(run_relay task add --title "Task C" | jq -r '.id')"

run_relay task publish "$A_ID" >/dev/null
run_relay task publish "$B_ID" >/dev/null
run_relay task publish "$C_ID" >/dev/null

echo "[1.1] deps add via CLI"
ADD_OUTPUT="$(run_relay task deps add "$A_ID" --on "$B_ID")"
assert_contains "$(echo "$ADD_OUTPUT" | jq -r '.dependencies | map(tostring) | join(",")')" "$B_ID" "A depends on B"

echo "[1.2] deps list via CLI"
LIST_OUTPUT="$(run_relay task deps list "$A_ID")"
assert_eq "1" "$(echo "$LIST_OUTPUT" | jq '.items | length')" "deps list shows 1 dependency"

echo "[1.3] deps remove via CLI"
REMOVE_OUTPUT="$(run_relay task deps remove "$A_ID" --on "$B_ID")"
assert_eq "0" "$(echo "$REMOVE_OUTPUT" | jq '.dependencies | length')" "A has no deps after remove"

echo "[1.4] deps set via CLI"
SET_OUTPUT="$(run_relay task deps set "$A_ID" --on "$B_ID" "$C_ID")"
assert_eq "2" "$(echo "$SET_OUTPUT" | jq '.dependencies | length')" "A has 2 deps after set"

# Clear and test cycle detection
run_relay task deps set "$A_ID" --on >/dev/null 2>&1 || true
run_relay task deps add "$A_ID" --on "$B_ID" >/dev/null
run_relay task deps add "$B_ID" --on "$C_ID" >/dev/null

echo "[1.5] cycle detection via CLI"
CYCLE_OUTPUT="$(run_relay task deps add "$C_ID" --on "$A_ID" 2>&1 || true)"
assert_contains "$CYCLE_OUTPUT" "cycle" "cycle detected for C→A"

echo "[1.6] self-dependency via CLI"
SELF_OUTPUT="$(run_relay task deps add "$A_ID" --on "$A_ID" 2>&1 || true)"
assert_contains "$SELF_OUTPUT" "itself" "self-dependency error"

echo "[1.7] deps add via direct curl"
run_relay task deps set "$A_ID" --on >/dev/null 2>&1 || true
CURL_RESULT="$(relay_post "/api/v1/projects/1/tasks/$A_ID/deps" "{\"dep_id\":$B_ID}")"
assert_contains "$(echo "$CURL_RESULT" | jq -r '.dependencies | map(tostring) | join(",")')" "$B_ID" "curl deps add works"

# ========================================
# Part 2: Relay-token mode (SENKO_SERVER_RELAY_TOKEN set)
# ========================================
echo "--- Part 2: deps via relay (relay token) ---"

stop_servers

UPSTREAM_PORT=$(allocate_port 0)
RELAY_PORT=$(allocate_port 1)
UPSTREAM_URL="http://127.0.0.1:$UPSTREAM_PORT"
RELAY_URL="http://127.0.0.1:$RELAY_PORT"

start_upstream
# Relay uses master key to authenticate with upstream
start_relay "$MASTER_KEY"
TEST_TOKEN=$(create_test_user_key "$UPSTREAM_URL" "$MASTER_KEY")

T1_ID="$(run_relay task add --title "Token Task 1" | jq -r '.id')"
T2_ID="$(run_relay task add --title "Token Task 2" | jq -r '.id')"

echo "[2.1] deps add with relay token"
DEP_OUT="$(run_relay task deps add "$T1_ID" --on "$T2_ID")"
assert_contains "$(echo "$DEP_OUT" | jq -r '.dependencies | map(tostring) | join(",")')" "$T2_ID" "deps add with relay token"

echo "[2.2] deps list with relay token"
DEP_LIST="$(run_relay task deps list "$T1_ID")"
assert_eq "1" "$(echo "$DEP_LIST" | jq '.items | length')" "deps list with relay token"

echo "[2.3] deps remove with relay token"
DEP_RM="$(run_relay task deps remove "$T1_ID" --on "$T2_ID")"
assert_eq "0" "$(echo "$DEP_RM" | jq '.dependencies | length')" "deps remove with relay token"

echo "[2.4] deps add via direct curl with relay token"
CURL_RT="$(relay_post "/api/v1/projects/1/tasks/$T1_ID/deps" "{\"dep_id\":$T2_ID}")"
assert_contains "$(echo "$CURL_RT" | jq -r '.dependencies | map(tostring) | join(",")')" "$T2_ID" "curl deps add with relay token"

test_summary
