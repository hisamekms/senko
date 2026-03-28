#!/usr/bin/env bash
# e2e test: inline hooks — fire-and-forget when CLI commands change task state

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: inline hooks ---"

# Helper: create config.toml with hooks (named hook format)
create_config() {
  local on_added="${1:-}"
  local on_completed="${2:-}"
  local config_dir="$TEST_PROJECT_ROOT/.senko"
  mkdir -p "$config_dir"
  : > "$config_dir/config.toml"
  if [[ -n "$on_added" ]]; then
    cat >> "$config_dir/config.toml" <<HOOKEOF
[hooks.on_task_added.default]
command = "$on_added"
HOOKEOF
  fi
  if [[ -n "$on_completed" ]]; then
    cat >> "$config_dir/config.toml" <<HOOKEOF
[hooks.on_task_completed.default]
command = "$on_completed"
HOOKEOF
  fi
}

# 1. on_task_added hook fires for new task
echo "[1] on_task_added hook fires"

HOOK_OUTPUT="$TEST_DIR/added_output.json"
create_config "cat > $HOOK_OUTPUT" ""

run_lf add --title "Hook Test Task" >/dev/null 2>&1

# Hooks are fire-and-forget child processes; wait briefly
wait_for "hook output created" 5 "[ -f '$HOOK_OUTPUT' ]"

if [[ -f "$HOOK_OUTPUT" ]]; then
  HOOK_JSON="$(cat "$HOOK_OUTPUT")"
  HOOK_EVENT="$(echo "$HOOK_JSON" | jq -r '.event')"
  HOOK_TITLE="$(echo "$HOOK_JSON" | jq -r '.task.title')"
  assert_eq "task_added" "$HOOK_EVENT" "on_task_added event type"
  assert_eq "Hook Test Task" "$HOOK_TITLE" "on_task_added task title"
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
run_lf ready "$TASK_ID" >/dev/null 2>&1
run_lf start "$TASK_ID" >/dev/null 2>&1

# Complete the task (should trigger on_task_completed inline)
run_lf complete "$TASK_ID" >/dev/null 2>&1

wait_for "completed hook output" 5 "[ -f '$HOOK_OUTPUT' ]"

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

# 3. No config file — commands run without error
echo "[3] No config file — commands run without error"

setup_test_env

OUTPUT="$(run_lf --output json add --title "No Config Task" 2>/dev/null)"
TITLE="$(echo "$OUTPUT" | jq -r '.title')"
assert_eq "No Config Task" "$TITLE" "task created without config"

# 4. JSON structure passed to hook
echo "[4] JSON structure in hook stdin"

setup_test_env

HOOK_OUTPUT="$TEST_DIR/json_check.json"
create_config "cat > $HOOK_OUTPUT" ""

run_lf add --title "JSON Check" --priority p1 >/dev/null 2>&1

wait_for "hook output created" 5 "[ -f '$HOOK_OUTPUT' ]"

if [[ -f "$HOOK_OUTPUT" ]]; then
  HAS_EVENT="$(jq 'has("event")' "$HOOK_OUTPUT")"
  HAS_TASK="$(jq 'has("task")' "$HOOK_OUTPUT")"
  TASK_HAS_ID="$(jq '.task | has("id")' "$HOOK_OUTPUT")"
  TASK_HAS_STATUS="$(jq '.task | has("status")' "$HOOK_OUTPUT")"
  HAS_STATS="$(jq 'has("stats")' "$HOOK_OUTPUT")"
  HAS_READY_COUNT="$(jq 'has("ready_count")' "$HOOK_OUTPUT")"

  assert_eq "true" "$HAS_EVENT" "JSON has event field"
  assert_eq "true" "$HAS_TASK" "JSON has task field"
  assert_eq "true" "$TASK_HAS_ID" "task has id field"
  assert_eq "true" "$TASK_HAS_STATUS" "task has status field"
  assert_eq "true" "$HAS_STATS" "JSON has stats field"
  assert_eq "true" "$HAS_READY_COUNT" "JSON has ready_count field"
else
  echo "  FAIL: hook output file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 5. Multiple hooks for same event
echo "[5] Multiple hooks for same event"

setup_test_env

MARKER1="$TEST_DIR/multi_hook1.txt"
MARKER2="$TEST_DIR/multi_hook2.txt"

config_dir="$TEST_PROJECT_ROOT/.senko"
mkdir -p "$config_dir"
cat > "$config_dir/config.toml" <<EOF
[hooks.on_task_added.hook1]
command = "echo hook1 > $MARKER1"

[hooks.on_task_added.hook2]
command = "echo hook2 > $MARKER2"
EOF

run_lf add --title "Multi Hook Task" >/dev/null 2>&1

wait_for "first marker" 5 "[ -f '$MARKER1' ]"
wait_for "second marker" 5 "[ -f '$MARKER2' ]"

if [[ -f "$MARKER1" ]]; then
  echo "  PASS: first hook executed"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: first hook marker not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

if [[ -f "$MARKER2" ]]; then
  echo "  PASS: second hook executed"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: second hook marker not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
