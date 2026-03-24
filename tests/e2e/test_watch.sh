#!/usr/bin/env bash
# e2e test: watch subcommand — foreground polling, hooks, daemon mode

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: watch subcommand ---"

# Helper: create config.toml with hooks
create_config() {
  local on_added="${1:-}"
  local on_completed="${2:-}"
  local config_dir="$TEST_PROJECT_ROOT/.localflow"
  mkdir -p "$config_dir"
  {
    echo "[hooks]"
    [[ -n "$on_added" ]] && echo "on_task_added = \"$on_added\""
    [[ -n "$on_completed" ]] && echo "on_task_completed = \"$on_completed\""
    true
  } > "$config_dir/config.toml"
}

# Helper: start watch in background, store PID
start_watch() {
  run_lf watch --interval 1 >/dev/null 2>&1 &
  WATCH_PID=$!
  sleep 2
}

# Helper: stop watch background process
stop_watch() {
  kill "$WATCH_PID" 2>/dev/null || true
  wait "$WATCH_PID" 2>/dev/null || true
}

# 1. on_task_added hook fires for new task
echo "[1] on_task_added hook fires"

HOOK_OUTPUT="$TEST_DIR/added_output.json"
create_config "cat > $HOOK_OUTPUT" ""

start_watch

# Add a task (this should trigger on_task_added)
run_lf add --title "Watch Test Task" >/dev/null 2>&1

# Wait for poll cycle
sleep 2
stop_watch

# Verify hook output
if [[ -f "$HOOK_OUTPUT" ]]; then
  HOOK_JSON="$(cat "$HOOK_OUTPUT")"
  HOOK_EVENT="$(echo "$HOOK_JSON" | jq -r '.event')"
  HOOK_TITLE="$(echo "$HOOK_JSON" | jq -r '.task.title')"
  assert_eq "task_added" "$HOOK_EVENT" "on_task_added event type"
  assert_eq "Watch Test Task" "$HOOK_TITLE" "on_task_added task title"
else
  echo "  FAIL: hook output file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 2. on_task_completed hook fires
echo "[2] on_task_completed hook fires"

setup_test_env

HOOK_OUTPUT="$TEST_DIR/completed_output.json"
create_config "" "cat > $HOOK_OUTPUT"

# Create and move task to in_progress first
TASK_ID="$(run_lf --output json add --title "Complete Me" 2>/dev/null | jq -r '.id')"
run_lf edit "$TASK_ID" --status todo >/dev/null 2>&1
run_lf edit "$TASK_ID" --status in-progress >/dev/null 2>&1

# Start watch (initial snapshot captures task as in_progress)
start_watch

# Complete the task (should trigger on_task_completed)
run_lf complete "$TASK_ID" >/dev/null 2>&1

# Wait for poll cycle
sleep 2
stop_watch

if [[ -f "$HOOK_OUTPUT" ]]; then
  HOOK_JSON="$(cat "$HOOK_OUTPUT")"
  HOOK_EVENT="$(echo "$HOOK_JSON" | jq -r '.event')"
  HOOK_TITLE="$(echo "$HOOK_JSON" | jq -r '.task.title')"
  assert_eq "task_completed" "$HOOK_EVENT" "on_task_completed event type"
  assert_eq "Complete Me" "$HOOK_TITLE" "on_task_completed task title"
else
  echo "  FAIL: hook output file not created for completed event"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 3. Daemon mode start/stop
echo "[3] Daemon start and stop"

setup_test_env
create_config "echo added" ""

PID_FILE="$TEST_PROJECT_ROOT/.localflow/watch.pid"

# Start daemon
run_lf watch -d --interval 1 >/dev/null 2>&1

sleep 1

# PID file should exist
if [[ -f "$PID_FILE" ]]; then
  echo "  PASS: PID file created"
  PASS_COUNT=$((PASS_COUNT + 1))

  DAEMON_PID="$(cat "$PID_FILE")"

  # Process should be running
  if kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "  PASS: daemon process is running"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: daemon process not running"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  # Stop daemon
  run_lf watch stop >/dev/null 2>&1

  # Wait for process to exit
  sleep 1

  # PID file should be removed
  if [[ ! -f "$PID_FILE" ]]; then
    echo "  PASS: PID file removed after stop"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: PID file still exists after stop"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  # Process should not be running
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "  PASS: daemon process stopped"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: daemon process still running"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    kill "$DAEMON_PID" 2>/dev/null || true
  fi
else
  echo "  FAIL: PID file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 4. No config file — watch runs without error (warns)
echo "[4] No config file — runs with warning"

setup_test_env

run_lf watch --interval 1 2>"$TEST_DIR/watch_stderr.log" &
WATCH_PID=$!
sleep 2
kill "$WATCH_PID" 2>/dev/null || true
wait "$WATCH_PID" 2>/dev/null || true

STDERR_OUTPUT="$(cat "$TEST_DIR/watch_stderr.log")"
assert_contains "$STDERR_OUTPUT" "no hooks configured" "warning shown when no hooks"

# 5. watch stop with no daemon running — error
echo "[5] watch stop with no daemon — error"

setup_test_env

STOP_OUTPUT="$(run_lf watch stop 2>&1 || true)"
assert_contains "$STOP_OUTPUT" "no watch daemon running" "error when no daemon to stop"

# 6. JSON structure passed to hook
echo "[6] JSON structure in hook stdin"

setup_test_env

HOOK_OUTPUT="$TEST_DIR/json_check.json"
create_config "cat > $HOOK_OUTPUT" ""

start_watch

run_lf add --title "JSON Check" --priority p1 >/dev/null 2>&1

sleep 2
stop_watch

if [[ -f "$HOOK_OUTPUT" ]]; then
  HAS_EVENT="$(jq 'has("event")' "$HOOK_OUTPUT")"
  HAS_TASK="$(jq 'has("task")' "$HOOK_OUTPUT")"
  TASK_HAS_ID="$(jq '.task | has("id")' "$HOOK_OUTPUT")"
  TASK_HAS_STATUS="$(jq '.task | has("status")' "$HOOK_OUTPUT")"

  assert_eq "true" "$HAS_EVENT" "JSON has event field"
  assert_eq "true" "$HAS_TASK" "JSON has task field"
  assert_eq "true" "$TASK_HAS_ID" "task has id field"
  assert_eq "true" "$TASK_HAS_STATUS" "task has status field"
else
  echo "  FAIL: hook output file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 7. watch status with no daemon — shows stopped
echo "[7] watch status with no daemon — stopped"

setup_test_env

STATUS_JSON="$(run_lf --output json watch status 2>&1)"
STATUS_VAL="$(echo "$STATUS_JSON" | jq -r '.status')"
assert_eq "stopped" "$STATUS_VAL" "status is stopped when no daemon"

# Verify no pid/interval/started_at fields in stopped state
HAS_PID="$(echo "$STATUS_JSON" | jq 'has("pid")')"
assert_eq "false" "$HAS_PID" "stopped status has no pid field"

# 8. watch status with daemon running — shows running
echo "[8] watch status with daemon running"

setup_test_env
create_config "echo added" ""

run_lf watch -d --interval 2 >/dev/null 2>&1
sleep 1

STATUS_JSON="$(run_lf --output json watch status 2>&1)"
STATUS_VAL="$(echo "$STATUS_JSON" | jq -r '.status')"
assert_eq "running" "$STATUS_VAL" "status is running when daemon active"

STATUS_PID="$(echo "$STATUS_JSON" | jq -r '.pid')"
if [[ "$STATUS_PID" =~ ^[0-9]+$ ]]; then
  echo "  PASS: pid is a number ($STATUS_PID)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: pid is not a number: $STATUS_PID"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

STATUS_INTERVAL="$(echo "$STATUS_JSON" | jq -r '.interval')"
assert_eq "2" "$STATUS_INTERVAL" "interval matches configured value"

HAS_STARTED="$(echo "$STATUS_JSON" | jq 'has("started_at")')"
assert_eq "true" "$HAS_STARTED" "running status has started_at field"

HAS_UPTIME="$(echo "$STATUS_JSON" | jq 'has("uptime_seconds")')"
assert_eq "true" "$HAS_UPTIME" "running status has uptime_seconds field"

# Clean up daemon
run_lf watch stop >/dev/null 2>&1

# 9. watch status text output
echo "[9] watch status text output"

setup_test_env

STATUS_TEXT="$(run_lf --output text watch status 2>&1)"
assert_contains "$STATUS_TEXT" "Status: stopped" "text output shows stopped"

# 10. Daemon mode creates default log file
echo "[10] Daemon creates default log file"

setup_test_env
create_config "echo added" ""

LOG_FILE="$TEST_PROJECT_ROOT/.localflow/watch.log"

# Start daemon
run_lf watch -d --interval 1 >/dev/null 2>&1

sleep 1

# Add a task to trigger an event
run_lf add --title "Log Test Task" >/dev/null 2>&1

# Wait for poll cycle
sleep 3

# Stop daemon
run_lf watch stop >/dev/null 2>&1

sleep 1

if [[ -f "$LOG_FILE" ]]; then
  echo "  PASS: default log file created"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: default log file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 11. Log file contains event entries
echo "[11] Log file contains event entries"

if [[ -f "$LOG_FILE" ]]; then
  LOG_CONTENT="$(cat "$LOG_FILE")"
  assert_contains "$LOG_CONTENT" "watch started" "log has watch started entry"
  assert_contains "$LOG_CONTENT" "event detected" "log has event detected entry"
  assert_contains "$LOG_CONTENT" "task_added" "log has task_added event"
else
  echo "  FAIL: log file missing, cannot check content"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 12. Custom --log-file path
echo "[12] Custom --log-file path"

setup_test_env
create_config "echo added" ""

CUSTOM_LOG="$TEST_DIR/custom_watch.log"

run_lf watch --interval 1 --log-file "$CUSTOM_LOG" >/dev/null 2>&1 &
WATCH_PID=$!
sleep 2

run_lf add --title "Custom Log Task" >/dev/null 2>&1

sleep 2
kill "$WATCH_PID" 2>/dev/null || true
wait "$WATCH_PID" 2>/dev/null || true

if [[ -f "$CUSTOM_LOG" ]]; then
  echo "  PASS: custom log file created"
  PASS_COUNT=$((PASS_COUNT + 1))
  CUSTOM_CONTENT="$(cat "$CUSTOM_LOG")"
  assert_contains "$CUSTOM_CONTENT" "watch started" "custom log has watch started"
else
  echo "  FAIL: custom log file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 13. Hook execution logged
echo "[13] Hook execution logged"

setup_test_env

HOOK_OUTPUT="$TEST_DIR/hook_log_output.json"
create_config "cat > $HOOK_OUTPUT" ""

CUSTOM_LOG="$TEST_DIR/hook_exec.log"

run_lf watch --interval 1 --log-file "$CUSTOM_LOG" >/dev/null 2>&1 &
WATCH_PID=$!
sleep 2

run_lf add --title "Hook Log Task" >/dev/null 2>&1

sleep 2
kill "$WATCH_PID" 2>/dev/null || true
wait "$WATCH_PID" 2>/dev/null || true

if [[ -f "$CUSTOM_LOG" ]]; then
  HOOK_LOG="$(cat "$CUSTOM_LOG")"
  assert_contains "$HOOK_LOG" "hook executed" "log has hook executed entry"
else
  echo "  FAIL: log file missing for hook check"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
