#!/usr/bin/env bash
# E2E tests for per-runtime hook firing under the new hooks schema.
#
# Verifies that hooks defined under [cli.*] fire only when the active runtime
# is `cli`, and hooks under [server.remote.*] fire only when the active runtime
# is `server.remote` (i.e. inside a `senko serve` process).

set -euo pipefail

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

SERVER_PID=""
MARKER_DIR="$TEST_DIR/hook-markers"
mkdir -p "$MARKER_DIR"

# --- Helper functions ---

write_config() {
  mkdir -p "$TEST_PROJECT_ROOT/.senko"
  cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_publish.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_task_publish"
mode = "sync"

[cli.task_start.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_task_start"
mode = "sync"

[cli.task_complete.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_task_complete"
mode = "sync"

[cli.task_cancel.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_task_cancel"
mode = "sync"

[server.remote.task_publish.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_task_publish"
mode = "sync"

[server.remote.task_start.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_task_start"
mode = "sync"

[server.remote.task_complete.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_task_complete"
mode = "sync"

[server.remote.task_cancel.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_task_cancel"
mode = "sync"
EOF
}

clear_markers() {
  rm -f "$MARKER_DIR"/*
}

MASTER_KEY=test-key

start_server() {
  PORT=$(allocate_port)
  API_URL="http://127.0.0.1:$PORT"
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" >/dev/null 2>&1 &
  SERVER_PID=$!
  wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"
  TEST_TOKEN=$(create_test_user_key "$API_URL" "$MASTER_KEY")
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
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

clear_hook_log() {
  run_lf hooks log --clear >/dev/null 2>&1 || true
}

# Count hook log entries matching runtime and event (event_fired entries).
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

assert_marker() {
  local marker="$1"
  local message="$2"
  if [[ -f "$MARKER_DIR/$marker" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message (missing marker: $marker)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_no_marker() {
  local marker="$1"
  local message="$2"
  if [[ ! -f "$MARKER_DIR/$marker" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message (unexpected marker: $marker)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

write_config

# ========================================
# Section 1: Direct CLI runtime (no server)
# Only [cli.*] hooks should fire.
# ========================================
echo "--- Section 1: direct CLI runtime ---"

clear_hook_log
clear_markers

T1=$(run_lf task add --title "Direct CLI transition" | jq -r '.id')
run_lf task publish "$T1" >/dev/null 2>&1
run_lf task start "$T1" >/dev/null 2>&1
run_lf task complete "$T1" >/dev/null 2>&1

sleep 1

echo "[1.1] cli hook for task_publish fired"
assert_marker "cli_task_publish" "direct cli: cli_task_publish marker created"

echo "[1.2] cli hook for task_start fired"
assert_marker "cli_task_start" "direct cli: cli_task_start marker created"

echo "[1.3] cli hook for task_complete fired"
assert_marker "cli_task_complete" "direct cli: cli_task_complete marker created"

echo "[1.4] server.remote hooks did NOT fire"
assert_no_marker "srv_task_publish" "direct cli: no srv_task_publish"
assert_no_marker "srv_task_start" "direct cli: no srv_task_start"
assert_no_marker "srv_task_complete" "direct cli: no srv_task_complete"

echo "[1.5] event_fired log entries under runtime=cli"
assert_gte "$(count_log_entries cli task_publish)" 1 "runtime=cli task_publish event_fired"
assert_gte "$(count_log_entries cli task_start)" 1 "runtime=cli task_start event_fired"
assert_gte "$(count_log_entries cli task_complete)" 1 "runtime=cli task_complete event_fired"

echo "[1.6] no server.remote event_fired entries from direct CLI"
assert_eq "0" "$(count_log_entries server.remote task_publish)" "direct cli: no server.remote task_publish event_fired"
assert_eq "0" "$(count_log_entries server.remote task_start)" "direct cli: no server.remote task_start event_fired"

# ========================================
# Section 2: server.remote runtime (via senko serve)
# [server.remote.*] hooks fire on the server side.
# ========================================
echo "--- Section 2: server.remote runtime ---"

start_server
clear_hook_log
clear_markers

T2=$(run_http task add --title "Server transition 1" | jq -r '.id')
run_http task publish "$T2" >/dev/null 2>&1
run_http task start "$T2" >/dev/null 2>&1
run_http task complete "$T2" >/dev/null 2>&1

T3=$(run_http task add --title "Server transition 2" | jq -r '.id')
run_http task publish "$T3" >/dev/null 2>&1
run_http task cancel "$T3" --reason "test cancel" >/dev/null 2>&1

sleep 1

echo "[2.1] server.remote hook for task_publish fired"
assert_marker "srv_task_publish" "server.remote: srv_task_publish marker created"

echo "[2.2] server.remote hook for task_start fired"
assert_marker "srv_task_start" "server.remote: srv_task_start marker created"

echo "[2.3] server.remote hook for task_complete fired"
assert_marker "srv_task_complete" "server.remote: srv_task_complete marker created"

echo "[2.4] server.remote hook for task_cancel fired"
assert_marker "srv_task_cancel" "server.remote: srv_task_cancel marker created"

echo "[2.5] server.remote event_fired log entries"
assert_gte "$(count_log_entries server.remote task_publish)" 1 "server.remote: task_publish event_fired"
assert_gte "$(count_log_entries server.remote task_start)" 1 "server.remote: task_start event_fired"
assert_gte "$(count_log_entries server.remote task_complete)" 1 "server.remote: task_complete event_fired"
assert_gte "$(count_log_entries server.remote task_cancel)" 1 "server.remote: task_cancel event_fired"

stop_server

test_summary
