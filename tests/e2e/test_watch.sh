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

test_summary
