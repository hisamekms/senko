#!/usr/bin/env bash
# e2e test: Task lifecycle (create → get → list → ready → start/next → complete/cancel)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Task Lifecycle ---"

# 1. Create a task
echo "[1] Task creation"
ADD_OUTPUT="$(run_lf --output json task add --title "Test Task")"
TASK_ID="$(echo "$ADD_OUTPUT" | jq -r '.id')"

assert_json_field "$ADD_OUTPUT" '.status' "draft" "new task status is draft"
assert_json_field "$ADD_OUTPUT" '.title' "Test Task" "new task title"
assert_contains "$TASK_ID" "" "task id is not empty"

# 2. Get task by ID
echo "[2] Task get"
GET_OUTPUT="$(run_lf --output json task get "$TASK_ID")"
assert_json_field "$GET_OUTPUT" '.title' "Test Task" "get returns correct title"
assert_json_field "$GET_OUTPUT" '.id' "$TASK_ID" "get returns correct id"

# 3. List tasks
echo "[3] Task list"
LIST_OUTPUT="$(run_lf --output json task list)"
LIST_CONTAINS_ID="$(echo "$LIST_OUTPUT" | jq -r --arg id "$TASK_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$LIST_CONTAINS_ID" "list contains created task"

# 4. Ready (draft → todo)
echo "[4] Ready: draft → todo"
READY_OUTPUT="$(run_lf --output json task publish "$TASK_ID")"
assert_json_field "$READY_OUTPUT" '.status' "todo" "ready sets status to todo"

# 5. Next task (transitions to in_progress via start)
echo "[5] Next task"
NEXT_OUTPUT="$(run_lf --output json task next)"
assert_json_field "$NEXT_OUTPUT" '.status' "in_progress" "next sets status to in_progress"
assert_json_field "$NEXT_OUTPUT" '.id' "$TASK_ID" "next returns our task"

STARTED_AT="$(echo "$NEXT_OUTPUT" | jq -r '.started_at')"
if [[ "$STARTED_AT" != "null" && -n "$STARTED_AT" ]]; then
  echo "  PASS: started_at is set ($STARTED_AT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: started_at should be set"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 6. Complete task
echo "[6] Complete task"
COMPLETE_OUTPUT="$(run_lf --output json task complete "$TASK_ID")"
assert_json_field "$COMPLETE_OUTPUT" '.status' "completed" "status is completed"

COMPLETED_AT="$(echo "$COMPLETE_OUTPUT" | jq -r '.completed_at')"
if [[ "$COMPLETED_AT" != "null" && -n "$COMPLETED_AT" ]]; then
  echo "  PASS: completed_at is set ($COMPLETED_AT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: completed_at should be set"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 7. Create another task, ready it, and cancel it
echo "[7] Cancel task"
ADD2_OUTPUT="$(run_lf --output json task add --title "Cancel Me")"
TASK2_ID="$(echo "$ADD2_OUTPUT" | jq -r '.id')"

run_lf task publish "$TASK2_ID" >/dev/null

CANCEL_OUTPUT="$(run_lf --output json task cancel "$TASK2_ID" --reason "不要")"
assert_json_field "$CANCEL_OUTPUT" '.status' "canceled" "cancel sets status to canceled"
assert_json_field "$CANCEL_OUTPUT" '.cancel_reason' "不要" "cancel_reason is set"

CANCELED_AT="$(echo "$CANCEL_OUTPUT" | jq -r '.canceled_at')"
if [[ "$CANCELED_AT" != "null" && -n "$CANCELED_AT" ]]; then
  echo "  PASS: canceled_at is set ($CANCELED_AT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: canceled_at should be set"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 8. Start with --session-id
echo "[8] Start with session-id"
ADD3_OUTPUT="$(run_lf --output json task add --title "Start Test")"
TASK3_ID="$(echo "$ADD3_OUTPUT" | jq -r '.id')"
run_lf task publish "$TASK3_ID" >/dev/null
START_OUTPUT="$(run_lf --output json task start "$TASK3_ID" --session-id "sess-123")"
assert_json_field "$START_OUTPUT" '.status' "in_progress" "start sets status to in_progress"
assert_json_field "$START_OUTPUT" '.assignee_session_id' "sess-123" "session_id is set"

test_summary
