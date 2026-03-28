#!/usr/bin/env bash
# e2e test: Inline hooks for all event types and from_status field

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Hook Events ---"

HOOK_LOG="$TEST_DIR/hook.log"

# Initialize DB first (creates .senko/)
run_lf --output json list >/dev/null 2>&1

# Configure hooks for all events (named hook format)
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[hooks.on_task_added.default]
command = "cat >> $HOOK_LOG"

[hooks.on_task_ready.default]
command = "cat >> $HOOK_LOG"

[hooks.on_task_started.default]
command = "cat >> $HOOK_LOG"

[hooks.on_task_completed.default]
command = "cat >> $HOOK_LOG"

[hooks.on_task_canceled.default]
command = "cat >> $HOOK_LOG"
EOF

# 1. Create a task → should fire task_added
echo "[1] task_added event"
TASK_ID="$(run_lf --output json add --title "Hook test" | jq -r '.id')"
wait_for "task_added event" 5 "grep -q '\"event\":\"task_added\"' '$HOOK_LOG'"

ADDED_EVENT="$(grep -c '"event":"task_added"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$ADDED_EVENT" -ge 1 ]; then
  echo "  PASS: task_added event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_added event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 2. Ready the task → should fire task_ready
echo "[2] task_ready event"
run_lf ready "$TASK_ID" >/dev/null
wait_for "task_ready event" 5 "grep -q '\"event\":\"task_ready\"' '$HOOK_LOG'"

READY_EVENT="$(grep -c '"event":"task_ready"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$READY_EVENT" -ge 1 ]; then
  echo "  PASS: task_ready event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_ready event not found in hook log"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Check from_status is present in task_ready event
READY_FROM="$(grep '"event":"task_ready"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"draft"' 2>/dev/null || echo 0)"
if [ "$READY_FROM" -ge 1 ]; then
  echo "  PASS: task_ready has from_status=draft"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_ready missing from_status=draft"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 3. Start the task → should fire task_started
echo "[3] task_started event"
run_lf start "$TASK_ID" >/dev/null
wait_for "task_started event" 5 "grep -q '\"event\":\"task_started\"' '$HOOK_LOG'"

STARTED_EVENT="$(grep -c '"event":"task_started"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$STARTED_EVENT" -ge 1 ]; then
  echo "  PASS: task_started event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_started event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

STARTED_FROM="$(grep '"event":"task_started"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"todo"' 2>/dev/null || echo 0)"
if [ "$STARTED_FROM" -ge 1 ]; then
  echo "  PASS: task_started has from_status=todo"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_started missing from_status=todo"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 4. Complete the task → should fire task_completed
echo "[4] task_completed event"
run_lf complete "$TASK_ID" >/dev/null
wait_for "task_completed event" 5 "grep -q '\"event\":\"task_completed\"' '$HOOK_LOG'"

COMPLETED_EVENT="$(grep -c '"event":"task_completed"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$COMPLETED_EVENT" -ge 1 ]; then
  echo "  PASS: task_completed event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_completed event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

COMPLETED_FROM="$(grep '"event":"task_completed"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"in_progress"' 2>/dev/null || echo 0)"
if [ "$COMPLETED_FROM" -ge 1 ]; then
  echo "  PASS: task_completed has from_status=in_progress"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_completed missing from_status=in_progress"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 5. Create and cancel a task → should fire task_canceled
echo "[5] task_canceled event"
TASK2_ID="$(run_lf --output json add --title "Cancel hook" | jq -r '.id')"
wait_for "task2 added event" 5 "grep -q 'Cancel hook' '$HOOK_LOG'"
run_lf cancel "$TASK2_ID" >/dev/null
wait_for "task_canceled event" 5 "grep -q '\"event\":\"task_canceled\"' '$HOOK_LOG'"

CANCELED_EVENT="$(grep -c '"event":"task_canceled"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$CANCELED_EVENT" -ge 1 ]; then
  echo "  PASS: task_canceled event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_canceled event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

CANCELED_FROM="$(grep '"event":"task_canceled"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"draft"' 2>/dev/null || echo 0)"
if [ "$CANCELED_FROM" -ge 1 ]; then
  echo "  PASS: task_canceled has from_status=draft"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_canceled missing from_status=draft"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 6. Unblocked tasks in completed event
echo "[6] unblocked_tasks in task_completed event"

setup_test_env

HOOK_LOG2="$TEST_DIR/hook2.log"

run_lf --output json list >/dev/null 2>&1

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[hooks.on_task_completed.default]
command = "cat >> $HOOK_LOG2"

[hooks.on_task_added.default]
command = "true"

[hooks.on_task_ready.default]
command = "true"

[hooks.on_task_started.default]
command = "true"
EOF

# Create task 1 and task 2 (depends on 1)
T1="$(run_lf --output json add --title "Blocker" | jq -r '.id')"
T2="$(run_lf --output json add --title "Blocked" --depends-on "$T1" | jq -r '.id')"
run_lf ready "$T1" >/dev/null
run_lf ready "$T2" >/dev/null
run_lf start "$T1" >/dev/null

# Complete task 1 → should unblock task 2
run_lf complete "$T1" >/dev/null
wait_for "completed event" 5 "[ -f '$HOOK_LOG2' ]"

if [ -f "$HOOK_LOG2" ]; then
  HAS_UNBLOCKED="$(grep '"event":"task_completed"' "$HOOK_LOG2" | head -1 | jq '.unblocked_tasks | length' 2>/dev/null || echo 0)"
  if [ "$HAS_UNBLOCKED" -ge 1 ]; then
    echo "  PASS: unblocked_tasks present in completed event"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: unblocked_tasks missing or empty"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  UNBLOCKED_TITLE="$(grep '"event":"task_completed"' "$HOOK_LOG2" | head -1 | jq -r '.unblocked_tasks[0].title' 2>/dev/null || echo "")"
  assert_eq "Blocked" "$UNBLOCKED_TITLE" "unblocked task is 'Blocked'"
else
  echo "  FAIL: hook log not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
