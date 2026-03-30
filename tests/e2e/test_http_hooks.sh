#!/usr/bin/env bash
# E2E tests for hook firing in HttpBackend mode with different HookMode settings.
# Verifies that hooks fire on the correct side (cli/api) for each HookMode
# across all state transitions (ready/start/complete/cancel).

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

SERVER_PID=""

# --- Helper functions ---

write_config() {
  local hook_mode="$1"
  mkdir -p "$TEST_PROJECT_ROOT/.senko"
  cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[backend]
hook_mode = "$hook_mode"

[hooks.on_task_ready.test_hook]
command = "true"
enabled = true

[hooks.on_task_started.test_hook]
command = "true"
enabled = true

[hooks.on_task_completed.test_hook]
command = "true"
enabled = true

[hooks.on_task_canceled.test_hook]
command = "true"
enabled = true
EOF
}

start_server() {
  PORT=$((20000 + RANDOM % 40000))
  API_URL="http://127.0.0.1:$PORT"
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" >/dev/null 2>&1 &
  SERVER_PID=$!
  wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"
}

stop_server() {
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
  fi
}

cleanup_test_env_full() {
  stop_server
  cleanup_test_env
}
trap cleanup_test_env_full EXIT

run_http() {
  SENKO_API_URL="$API_URL" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

clear_hook_log() {
  run_lf hooks log --clear >/dev/null 2>&1 || true
}

# Count hook log entries matching runtime and event.
# Uses event_fired entries (logged for every hook fire, even with 0 hooks configured).
count_log_entries() {
  local runtime="$1"
  local event="$2"
  local log_file="$XDG_STATE_HOME/senko/hooks.log"
  if [[ ! -f "$log_file" ]]; then
    echo "0"
    return
  fi
  jq -s "[.[] | select(.runtime == \"$runtime\" and .event == \"$event\" and .type == \"event_fired\")] | length" < "$log_file"
}

# Run all four transitions: ready, start, complete (task 1), ready + cancel (task 2)
run_transitions() {
  local t1
  t1=$(run_http add --title "Transition task 1")
  local t1_id
  t1_id=$(echo "$t1" | jq -r '.id')

  run_http ready "$t1_id" >/dev/null 2>&1
  run_http start "$t1_id" >/dev/null 2>&1
  run_http complete "$t1_id" >/dev/null 2>&1

  local t2
  t2=$(run_http add --title "Transition task 2")
  local t2_id
  t2_id=$(echo "$t2" | jq -r '.id')

  run_http ready "$t2_id" >/dev/null 2>&1
  run_http cancel "$t2_id" --reason "test cancel" >/dev/null 2>&1
}

assert_gte() {
  local actual="$1"
  local threshold="$2"
  local message="$3"
  if [[ "$actual" -ge "$threshold" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message"
    echo "    expected: >= $threshold"
    echo "    actual:   $actual"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

# ========================================
# Section 1: HookMode = server (default)
# CLI should NOT fire hooks, API should fire hooks
# ========================================
echo "--- Section 1: HookMode = server ---"

write_config "server"
start_server
clear_hook_log

run_transitions
sleep 1

echo "[1.1] Server mode: API fires task_ready"
assert_gte "$(count_log_entries api task_ready)" 1 "server: api fires task_ready"

echo "[1.2] Server mode: API fires task_started"
assert_gte "$(count_log_entries api task_started)" 1 "server: api fires task_started"

echo "[1.3] Server mode: API fires task_completed"
assert_gte "$(count_log_entries api task_completed)" 1 "server: api fires task_completed"

echo "[1.4] Server mode: API fires task_canceled"
assert_gte "$(count_log_entries api task_canceled)" 1 "server: api fires task_canceled"

echo "[1.5] Server mode: CLI does NOT fire task_ready"
assert_eq "0" "$(count_log_entries cli task_ready)" "server: cli no task_ready"

echo "[1.6] Server mode: CLI does NOT fire task_started"
assert_eq "0" "$(count_log_entries cli task_started)" "server: cli no task_started"

echo "[1.7] Server mode: CLI does NOT fire task_completed"
assert_eq "0" "$(count_log_entries cli task_completed)" "server: cli no task_completed"

echo "[1.8] Server mode: CLI does NOT fire task_canceled"
assert_eq "0" "$(count_log_entries cli task_canceled)" "server: cli no task_canceled"

stop_server

# ========================================
# Section 2: HookMode = client
# CLI should fire hooks (API also fires)
# ========================================
echo "--- Section 2: HookMode = client ---"

write_config "client"
start_server
clear_hook_log

run_transitions
sleep 1

echo "[2.1] Client mode: CLI fires task_ready"
assert_gte "$(count_log_entries cli task_ready)" 1 "client: cli fires task_ready"

echo "[2.2] Client mode: CLI fires task_started"
assert_gte "$(count_log_entries cli task_started)" 1 "client: cli fires task_started"

echo "[2.3] Client mode: CLI fires task_completed"
assert_gte "$(count_log_entries cli task_completed)" 1 "client: cli fires task_completed"

echo "[2.4] Client mode: CLI fires task_canceled"
assert_gte "$(count_log_entries cli task_canceled)" 1 "client: cli fires task_canceled"

stop_server

# ========================================
# Section 3: HookMode = both
# Both CLI and API should fire hooks
# ========================================
echo "--- Section 3: HookMode = both ---"

write_config "both"
start_server
clear_hook_log

run_transitions
sleep 1

echo "[3.1] Both mode: CLI fires task_ready"
assert_gte "$(count_log_entries cli task_ready)" 1 "both: cli fires task_ready"

echo "[3.2] Both mode: CLI fires task_started"
assert_gte "$(count_log_entries cli task_started)" 1 "both: cli fires task_started"

echo "[3.3] Both mode: CLI fires task_completed"
assert_gte "$(count_log_entries cli task_completed)" 1 "both: cli fires task_completed"

echo "[3.4] Both mode: CLI fires task_canceled"
assert_gte "$(count_log_entries cli task_canceled)" 1 "both: cli fires task_canceled"

echo "[3.5] Both mode: API fires task_ready"
assert_gte "$(count_log_entries api task_ready)" 1 "both: api fires task_ready"

echo "[3.6] Both mode: API fires task_started"
assert_gte "$(count_log_entries api task_started)" 1 "both: api fires task_started"

echo "[3.7] Both mode: API fires task_completed"
assert_gte "$(count_log_entries api task_completed)" 1 "both: api fires task_completed"

echo "[3.8] Both mode: API fires task_canceled"
assert_gte "$(count_log_entries api task_canceled)" 1 "both: api fires task_canceled"

stop_server

test_summary
