#!/usr/bin/env bash
# E2E tests for the serve (JSON API) subcommand
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

# Pick a random high port
PORT=$((20000 + RANDOM % 40000))
BASE="http://127.0.0.1:$PORT/api/v1"
PBASE="$BASE/projects/1"

# Start the API server in background
"$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

# Wait for server to be ready
wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

# --- Helpers ---
# GET request
api_get() {
  curl -sf "$@"
}
# POST/PUT/DELETE with JSON body
api_json() {
  curl -sf -H "Content-Type: application/json" "$@"
}
# Get HTTP status code
api_status() {
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" "$@"
}

echo "=== Stats endpoint ==="
STATS=$(api_get "$BASE/projects/1/stats")
assert_eq "0" "$(echo "$STATS" | jq 'length')" "stats is empty initially"

echo ""
echo "=== Config endpoint ==="
CONFIG=$(api_get "$BASE/config")
assert_json_field "$CONFIG" '.workflow.completion_mode' "merge_then_complete" "default completion_mode"
assert_json_field "$CONFIG" '.workflow.auto_merge' "true" "default auto_merge"

echo ""
echo "=== Create task ==="
TASK1=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Task One","description":"First task"}')
assert_json_field "$TASK1" '.title' "Task One" "created task title"
assert_json_field "$TASK1" '.status' "draft" "created task status is draft"
assert_json_field "$TASK1" '.description' "First task" "created task description"
TASK1_ID=$(echo "$TASK1" | jq -r '.id')

echo ""
echo "=== Get task ==="
GOT=$(api_get "$PBASE/tasks/$TASK1_ID")
assert_json_field "$GOT" '.id' "$TASK1_ID" "get task by id"
assert_json_field "$GOT" '.title' "Task One" "get task title"

echo ""
echo "=== Create second task ==="
TASK2=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Task Two","priority":"P1","tags":["backend"],"definition_of_done":["Write tests","Deploy"]}')
TASK2_ID=$(echo "$TASK2" | jq -r '.id')
assert_json_field "$TASK2" '.priority' "P1" "task2 priority"
assert_eq "2" "$(echo "$TASK2" | jq '.definition_of_done | length')" "task2 has 2 DoD items"

echo ""
echo "=== List tasks ==="
LIST=$(api_get "$PBASE/tasks")
assert_eq "2" "$(echo "$LIST" | jq 'length')" "list returns 2 tasks"

echo ""
echo "=== Edit task ==="
EDITED=$(api_json -X PUT "$PBASE/tasks/$TASK1_ID" -d '{"title":"Task One Updated","add_tags":["frontend"]}')
assert_json_field "$EDITED" '.title' "Task One Updated" "edited title"
assert_contains "$(echo "$EDITED" | jq -r '.tags[]')" "frontend" "edited tags contains frontend"

echo ""
echo "=== Ready task ==="
READY=$(api_json -X POST "$PBASE/tasks/$TASK1_ID/ready" -d '{}')
assert_json_field "$READY" '.status' "todo" "ready transitions to todo"

echo ""
echo "=== Ready task2 ==="
api_json -X POST "$PBASE/tasks/$TASK2_ID/ready" -d '{}' >/dev/null

echo ""
echo "=== Start task ==="
STARTED=$(api_json -X POST "$PBASE/tasks/$TASK1_ID/start" -d '{"session_id":"test-session"}')
assert_json_field "$STARTED" '.status' "in_progress" "start transitions to in_progress"
assert_json_field "$STARTED" '.assignee_session_id' "test-session" "session_id set"

echo ""
echo "=== Complete task (no DoD) ==="
COMPLETED=$(api_json -X POST "$PBASE/tasks/$TASK1_ID/complete" -d '{}')
assert_json_field "$COMPLETED" '.task.status' "completed" "complete transitions to completed"

echo ""
echo "=== List filtered by status ==="
LIST_TODO=$(api_get "$PBASE/tasks?status=todo")
assert_eq "1" "$(echo "$LIST_TODO" | jq 'length')" "1 todo task"

echo ""
echo "=== Stats after operations ==="
STATS2=$(api_get "$BASE/projects/1/stats")
assert_json_field "$STATS2" '.completed' "1" "1 completed in stats"
assert_json_field "$STATS2" '.todo' "1" "1 todo in stats"

echo ""
echo "=== Start task2 ==="
api_json -X POST "$PBASE/tasks/$TASK2_ID/start" -d '{}' >/dev/null

echo ""
echo "=== Complete with unchecked DoD should fail ==="
STATUS=$(api_status -X POST "$PBASE/tasks/$TASK2_ID/complete" -d '{}')
assert_eq "409" "$STATUS" "complete with unchecked DoD returns 409"

echo ""
echo "=== DoD check ==="
DOD_CHECKED=$(api_json -X POST "$PBASE/tasks/$TASK2_ID/dod/1/check" -d '{}')
assert_eq "true" "$(echo "$DOD_CHECKED" | jq '.definition_of_done[0].checked')" "DoD item 1 checked"

echo ""
echo "=== DoD uncheck ==="
DOD_UNCHECKED=$(api_json -X POST "$PBASE/tasks/$TASK2_ID/dod/1/uncheck" -d '{}')
assert_eq "false" "$(echo "$DOD_UNCHECKED" | jq '.definition_of_done[0].checked')" "DoD item 1 unchecked"

echo ""
echo "=== Check all DoD and complete ==="
api_json -X POST "$PBASE/tasks/$TASK2_ID/dod/1/check" -d '{}' >/dev/null
api_json -X POST "$PBASE/tasks/$TASK2_ID/dod/2/check" -d '{}' >/dev/null
COMPLETED2=$(api_json -X POST "$PBASE/tasks/$TASK2_ID/complete" -d '{}')
assert_json_field "$COMPLETED2" '.task.status' "completed" "complete after checking all DoD"

echo ""
echo "=== Create tasks for deps test ==="
TASK3=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Dep Parent"}')
TASK3_ID=$(echo "$TASK3" | jq -r '.id')
TASK4=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Dep Child"}')
TASK4_ID=$(echo "$TASK4" | jq -r '.id')

echo ""
echo "=== Add dependency ==="
DEP_ADDED=$(api_json -X POST "$PBASE/tasks/$TASK4_ID/deps" -d "{\"dep_id\":$TASK3_ID}")
assert_contains "$(echo "$DEP_ADDED" | jq -r '.dependencies[]')" "$TASK3_ID" "dependency added"

echo ""
echo "=== List dependencies ==="
DEPS=$(api_get "$PBASE/tasks/$TASK4_ID/deps")
assert_eq "1" "$(echo "$DEPS" | jq 'length')" "1 dependency"
assert_json_field "$(echo "$DEPS" | jq '.[0]')" '.id' "$TASK3_ID" "dep is task3"

echo ""
echo "=== Remove dependency ==="
DEP_REMOVED=$(api_json -X DELETE "$PBASE/tasks/$TASK4_ID/deps/$TASK3_ID")
assert_eq "0" "$(echo "$DEP_REMOVED" | jq '.dependencies | length')" "dependency removed"

echo ""
echo "=== Cancel task ==="
api_json -X POST "$PBASE/tasks/$TASK3_ID/ready" -d '{}' >/dev/null
CANCELED=$(api_json -X POST "$PBASE/tasks/$TASK3_ID/cancel" -d '{"reason":"no longer needed"}')
assert_json_field "$CANCELED" '.status' "canceled" "cancel transitions to canceled"
assert_json_field "$CANCELED" '.cancel_reason' "no longer needed" "cancel reason set"

echo ""
echo "=== Next task (auto-select) ==="
# Create and ready a task for next to pick
TASK5=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Next Candidate","priority":"P0"}')
TASK5_ID=$(echo "$TASK5" | jq -r '.id')
api_json -X POST "$PBASE/tasks/$TASK5_ID/ready" -d '{}' >/dev/null

NEXT=$(api_json -X POST "$PBASE/tasks/next" -d '{}')
assert_json_field "$NEXT" '.status' "in_progress" "next auto-starts task"
assert_json_field "$NEXT" '.title' "Next Candidate" "next picks highest priority"

echo ""
echo "=== Delete task ==="
# Create a task to delete
TASK6=$(api_json -X POST "$PBASE/tasks" -d '{"title":"To Delete"}')
TASK6_ID=$(echo "$TASK6" | jq -r '.id')
DEL_STATUS=$(api_status -X DELETE "$PBASE/tasks/$TASK6_ID")
assert_eq "204" "$DEL_STATUS" "delete returns 204"
# Verify it's gone
GET_DEL_STATUS=$(api_status "$PBASE/tasks/$TASK6_ID")
assert_eq "404" "$GET_DEL_STATUS" "deleted task returns 404"

echo ""
echo "=== Error: get nonexistent task ==="
STATUS_404=$(api_status "$PBASE/tasks/99999")
assert_eq "404" "$STATUS_404" "nonexistent task returns 404"

echo ""
echo "=== Error: invalid status transition ==="
# Try to complete a draft task
TASK7=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Draft Task"}')
TASK7_ID=$(echo "$TASK7" | jq -r '.id')
STATUS_409=$(api_status -X POST "$PBASE/tasks/$TASK7_ID/complete" -d '{}')
assert_eq "409" "$STATUS_409" "complete draft returns 409"

echo ""
echo "=== Error: invalid status filter ==="
STATUS_400=$(api_status "$PBASE/tasks?status=invalid_status")
assert_eq "400" "$STATUS_400" "invalid status filter returns 400"

echo ""
echo "=== Create task with branch template ==="
TASK8=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Branch Task","branch":"feature/${task_id}-test"}')
TASK8_ID=$(echo "$TASK8" | jq -r '.id')
BRANCH=$(echo "$TASK8" | jq -r '.branch')
assert_eq "feature/${TASK8_ID}-test" "$BRANCH" "branch template expanded"

echo ""
echo "=== Next when no eligible task ==="
# Complete or cancel remaining active tasks
api_json -X POST "$PBASE/tasks/$TASK5_ID/complete" -d '{}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK4_ID/ready" -d '{}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK4_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK7_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK8_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
STATUS_NEXT_EMPTY=$(api_status -X POST "$PBASE/tasks/next" -d '{}')
assert_eq "404" "$STATUS_NEXT_EMPTY" "next with no eligible task returns 404"

test_summary
