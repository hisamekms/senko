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
run_lf --output json task list >/dev/null 2>&1

# Configure hooks for all events under the new [cli.<action>.hooks.*] schema.
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_add.hooks.default]
command = "cat >> $HOOK_LOG"

[cli.task_publish.hooks.default]
command = "cat >> $HOOK_LOG"

[cli.task_start.hooks.default]
command = "cat >> $HOOK_LOG"

[cli.task_complete.hooks.default]
command = "cat >> $HOOK_LOG"

[cli.task_cancel.hooks.default]
command = "cat >> $HOOK_LOG"
EOF

# 1. Create a task → should fire task_add
echo "[1] task_add event"
TASK_ID="$(run_lf --output json task add --title "Hook test" | jq -r '.id')"
wait_for "task_add event" 5 "grep -q '\"event\":\"task_add\"' '$HOOK_LOG'"

ADDED_EVENT="$(grep -c '"event":"task_add"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$ADDED_EVENT" -ge 1 ]; then
  echo "  PASS: task_add event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_add event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# task_add is emitted post-transition with status=draft → is_ready must be false.
# Events are appended as concatenated JSON (no newline between objects), so we
# parse the stream with jq and select by the inner event name.
ADDED_IS_READY="$(jq -r 'select(.event.event == "task_add") | .event.is_ready' "$HOOK_LOG" 2>/dev/null | head -1)"
assert_eq "false" "$ADDED_IS_READY" "task_add has is_ready=false (draft)"

# 2. Publish the task → should fire task_publish
echo "[2] task_publish event"
run_lf task publish "$TASK_ID" >/dev/null
wait_for "task_publish event" 5 "grep -q '\"event\":\"task_publish\"' '$HOOK_LOG'"

READY_EVENT="$(grep -c '"event":"task_publish"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$READY_EVENT" -ge 1 ]; then
  echo "  PASS: task_publish event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_publish event not found in hook log"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Check from_status is present in task_publish event
READY_FROM="$(grep '"event":"task_publish"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"draft"' 2>/dev/null || echo 0)"
if [ "$READY_FROM" -ge 1 ]; then
  echo "  PASS: task_publish has from_status=draft"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_publish missing from_status=draft"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# task_publish is emitted post-transition with status=todo and no deps → is_ready must be true
READY_IS_READY="$(jq -r 'select(.event.event == "task_publish") | .event.is_ready' "$HOOK_LOG" 2>/dev/null | head -1)"
assert_eq "true" "$READY_IS_READY" "task_publish has is_ready=true (todo, no deps)"

# 3. Start the task → should fire task_start
echo "[3] task_start event"
run_lf task start "$TASK_ID" >/dev/null
wait_for "task_start event" 5 "grep -q '\"event\":\"task_start\"' '$HOOK_LOG'"

STARTED_EVENT="$(grep -c '"event":"task_start"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$STARTED_EVENT" -ge 1 ]; then
  echo "  PASS: task_start event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_start event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

STARTED_FROM="$(grep '"event":"task_start"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"todo"' 2>/dev/null || echo 0)"
if [ "$STARTED_FROM" -ge 1 ]; then
  echo "  PASS: task_start has from_status=todo"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_start missing from_status=todo"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# task_start is emitted post-transition with status=in_progress → is_ready must be false
STARTED_IS_READY="$(jq -r 'select(.event.event == "task_start") | .event.is_ready' "$HOOK_LOG" 2>/dev/null | head -1)"
assert_eq "false" "$STARTED_IS_READY" "task_start has is_ready=false (in_progress)"

# 4. Complete the task → should fire task_complete
echo "[4] task_complete event"
run_lf task complete "$TASK_ID" >/dev/null
wait_for "task_complete event" 5 "grep -q '\"event\":\"task_complete\"' '$HOOK_LOG'"

COMPLETED_EVENT="$(grep -c '"event":"task_complete"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$COMPLETED_EVENT" -ge 1 ]; then
  echo "  PASS: task_complete event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_complete event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

COMPLETED_FROM="$(grep '"event":"task_complete"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"in_progress"' 2>/dev/null || echo 0)"
if [ "$COMPLETED_FROM" -ge 1 ]; then
  echo "  PASS: task_complete has from_status=in_progress"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_complete missing from_status=in_progress"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# task_complete is emitted post-transition with status=completed → is_ready must be false
COMPLETED_IS_READY="$(jq -r 'select(.event.event == "task_complete") | .event.is_ready' "$HOOK_LOG" 2>/dev/null | head -1)"
assert_eq "false" "$COMPLETED_IS_READY" "task_complete has is_ready=false (completed)"

# 5. Create and cancel a task → should fire task_cancel
echo "[5] task_cancel event"
TASK2_ID="$(run_lf --output json task add --title "Cancel hook" | jq -r '.id')"
wait_for "task2 added event" 5 "grep -q 'Cancel hook' '$HOOK_LOG'"
run_lf task cancel "$TASK2_ID" >/dev/null
wait_for "task_cancel event" 5 "grep -q '\"event\":\"task_cancel\"' '$HOOK_LOG'"

CANCELED_EVENT="$(grep -c '"event":"task_cancel"' "$HOOK_LOG" 2>/dev/null || echo 0)"
if [ "$CANCELED_EVENT" -ge 1 ]; then
  echo "  PASS: task_cancel event fired"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_cancel event not found"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

CANCELED_FROM="$(grep '"event":"task_cancel"' "$HOOK_LOG" | head -1 | grep -c '"from_status":"draft"' 2>/dev/null || echo 0)"
if [ "$CANCELED_FROM" -ge 1 ]; then
  echo "  PASS: task_cancel has from_status=draft"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task_cancel missing from_status=draft"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# task_cancel is emitted post-transition with status=canceled → is_ready must be false
CANCELED_IS_READY="$(jq -r 'select(.event.event == "task_cancel") | .event.is_ready' "$HOOK_LOG" 2>/dev/null | head -1)"
assert_eq "false" "$CANCELED_IS_READY" "task_cancel has is_ready=false (canceled)"

# 6. Unblocked tasks in task_complete event
echo "[6] unblocked_tasks in task_complete event"

setup_test_env

HOOK_LOG2="$TEST_DIR/hook2.log"

run_lf --output json task list >/dev/null 2>&1

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_complete.hooks.default]
command = "cat >> $HOOK_LOG2"

[cli.task_add.hooks.default]
command = "true"

[cli.task_publish.hooks.default]
command = "true"

[cli.task_start.hooks.default]
command = "true"
EOF

# Create task 1 and task 2 (depends on 1)
T1="$(run_lf --output json task add --title "Blocker" | jq -r '.id')"
T2="$(run_lf --output json task add --title "Blocked" --depends-on "$T1" | jq -r '.id')"
run_lf task publish "$T1" >/dev/null
run_lf task publish "$T2" >/dev/null
run_lf task start "$T1" >/dev/null

# Complete task 1 → should unblock task 2
run_lf task complete "$T1" >/dev/null
wait_for "completed event" 5 "[ -f '$HOOK_LOG2' ]"

if [ -f "$HOOK_LOG2" ]; then
  HAS_UNBLOCKED="$(grep '"event":"task_complete"' "$HOOK_LOG2" | head -1 | jq '.event.unblocked_tasks | length' 2>/dev/null || echo 0)"
  if [ "$HAS_UNBLOCKED" -ge 1 ]; then
    echo "  PASS: unblocked_tasks present in completed event"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: unblocked_tasks missing or empty"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  UNBLOCKED_TITLE="$(grep '"event":"task_complete"' "$HOOK_LOG2" | head -1 | jq -r '.event.unblocked_tasks[0].title' 2>/dev/null || echo "")"
  assert_eq "Blocked" "$UNBLOCKED_TITLE" "unblocked task is 'Blocked'"
else
  echo "  FAIL: hook log not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
