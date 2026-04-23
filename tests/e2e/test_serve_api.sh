#!/usr/bin/env bash
# E2E tests for the serve (JSON API) subcommand
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
BASE="http://127.0.0.1:$PORT/api/v1"
PBASE="$BASE/projects/1"

# Start the API server in background
MASTER_KEY=test-key
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

# Wait for server to be ready
wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

# Create a real user and API key (master key is only for user creation)
TEST_TOKEN=$(create_test_user_key "http://127.0.0.1:$PORT" "$MASTER_KEY")

# --- Helpers ---
# GET request
api_get() {
  curl -sf -H "Authorization: Bearer $TEST_TOKEN" "$@"
}
# POST/PUT/DELETE with JSON body
api_json() {
  curl -sf -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}
# Get HTTP status code
api_status() {
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

echo "=== Stats endpoint ==="
STATS=$(api_get "$BASE/projects/1/stats")
assert_eq "0" "$(echo "$STATS" | jq 'length')" "stats is empty initially"

echo ""
echo "=== Config endpoint ==="
CONFIG=$(api_get "$BASE/config")
assert_json_field "$CONFIG" '.workflow.merge_via' "direct" "default merge_via"
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
assert_eq "2" "$(echo "$LIST" | jq '.items | length')" "list returns 2 tasks"

echo ""
echo "=== Edit task ==="
EDITED=$(api_json -X PUT "$PBASE/tasks/$TASK1_ID" -d '{"title":"Task One Updated","add_tags":["frontend"]}')
assert_json_field "$EDITED" '.title' "Task One Updated" "edited title"
assert_contains "$(echo "$EDITED" | jq -r '.tags[]')" "frontend" "edited tags contains frontend"

echo ""
echo "=== Publish task ==="
READY=$(api_json -X POST "$PBASE/tasks/$TASK1_ID/publish" -d '{}')
assert_json_field "$READY" '.status' "todo" "ready transitions to todo"

echo ""
echo "=== Publish task2 ==="
api_json -X POST "$PBASE/tasks/$TASK2_ID/publish" -d '{}' >/dev/null

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
assert_eq "1" "$(echo "$LIST_TODO" | jq '.items | length')" "1 todo task"

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
assert_eq "1" "$(echo "$DEPS" | jq '.items | length')" "1 dependency"
assert_json_field "$(echo "$DEPS" | jq '.items[0]')" '.id' "$TASK3_ID" "dep is task3"

echo ""
echo "=== Remove dependency ==="
DEP_REMOVED=$(api_json -X DELETE "$PBASE/tasks/$TASK4_ID/deps/$TASK3_ID")
assert_eq "0" "$(echo "$DEP_REMOVED" | jq '.dependencies | length')" "dependency removed"

echo ""
echo "=== Cancel task ==="
api_json -X POST "$PBASE/tasks/$TASK3_ID/publish" -d '{}' >/dev/null
CANCELED=$(api_json -X POST "$PBASE/tasks/$TASK3_ID/cancel" -d '{"reason":"no longer needed"}')
assert_json_field "$CANCELED" '.status' "canceled" "cancel transitions to canceled"
assert_json_field "$CANCELED" '.cancel_reason' "no longer needed" "cancel reason set"

echo ""
echo "=== Next task: add(assignee=self, DoD) → ready → next → dod check → complete ==="
TASK5=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Next Candidate","priority":"P0","assignee_user_id":"self","definition_of_done":["Next DoD"]}')
TASK5_ID=$(echo "$TASK5" | jq -r '.id')
api_json -X POST "$PBASE/tasks/$TASK5_ID/publish" -d '{}' >/dev/null

NEXT=$(api_json -X POST "$PBASE/tasks/next" -d '{}')
assert_json_field "$NEXT" '.status' "in_progress" "next auto-starts task"
assert_json_field "$NEXT" '.title' "Next Candidate" "next picks highest priority"

api_json -X POST "$PBASE/tasks/$TASK5_ID/dod/1/check" -d '{}' >/dev/null
api_json -X POST "$PBASE/tasks/$TASK5_ID/complete" -d '{}' >/dev/null

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
echo "=== Get project ==="
PROJECT=$(api_get "$BASE/projects/1")
assert_json_field "$PROJECT" '.id' "1" "get project by id"
assert_json_field "$PROJECT" '.name' "default" "project name is default"

echo ""
echo "=== Get nonexistent project ==="
STATUS_PROJ_403=$(api_status "$BASE/projects/99999")
assert_eq "403" "$STATUS_PROJ_403" "nonexistent project returns 403 (no permission)"

echo ""
echo "=== Preview transition: draft to todo (allowed) ==="
PT_TASK=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Preview Test"}')
PT_TASK_ID=$(echo "$PT_TASK" | jq -r '.id')
PREVIEW_OK=$(api_get "$PBASE/tasks/$PT_TASK_ID/preview-transition?target=todo")
assert_json_field "$PREVIEW_OK" '.allowed' "true" "preview draft->todo allowed"
assert_json_field "$PREVIEW_OK" '.target_status' "todo" "preview target_status is todo"

echo ""
echo "=== Preview transition: draft to completed (not allowed) ==="
PREVIEW_NG=$(api_get "$PBASE/tasks/$PT_TASK_ID/preview-transition?target=completed")
assert_json_field "$PREVIEW_NG" '.allowed' "false" "preview draft->completed not allowed"

echo ""
echo "=== Preview transition: unchecked DoD blocks complete ==="
PT_DOD=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Preview DoD","definition_of_done":["Check me"]}')
PT_DOD_ID=$(echo "$PT_DOD" | jq -r '.id')
api_json -X POST "$PBASE/tasks/$PT_DOD_ID/publish" -d '{}' >/dev/null
api_json -X POST "$PBASE/tasks/$PT_DOD_ID/start" -d '{}' >/dev/null
PREVIEW_DOD_RESULT=$(api_get "$PBASE/tasks/$PT_DOD_ID/preview-transition?target=completed")
assert_json_field "$PREVIEW_DOD_RESULT" '.allowed' "false" "preview complete with unchecked DoD not allowed"
assert_contains "$(echo "$PREVIEW_DOD_RESULT" | jq -r '.reason')" "DoD" "preview reason mentions DoD"

echo ""
echo "=== Preview next: has candidate ==="
api_json -X POST "$PBASE/tasks/$PT_TASK_ID/publish" -d '{}' >/dev/null
PREVIEW_NEXT=$(api_get "$PBASE/tasks/preview-next")
assert_json_field "$PREVIEW_NEXT" '.allowed' "true" "preview-next has candidate"
assert_json_field "$PREVIEW_NEXT" '.target_status' "in_progress" "preview-next target is in_progress"

echo ""
echo "=== Save task (_save) ==="
SAVE_TASK=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Save Target","description":"original desc"}')
SAVE_TASK_ID=$(echo "$SAVE_TASK" | jq -r '.id')
# Get full task JSON, modify title
SAVE_BODY=$(api_get "$PBASE/tasks/$SAVE_TASK_ID" | jq '.title = "Save Target Updated"')
SAVE_STATUS=$(api_status -X PUT "$PBASE/tasks/$SAVE_TASK_ID/_save" -d "$SAVE_BODY")
assert_eq "204" "$SAVE_STATUS" "save task returns 204"
# Verify the change persisted
SAVE_VERIFY=$(api_get "$PBASE/tasks/$SAVE_TASK_ID")
assert_json_field "$SAVE_VERIFY" '.title' "Save Target Updated" "save task persisted title"
assert_json_field "$SAVE_VERIFY" '.description' "original desc" "save task kept description"

echo ""
echo "=== Save task ID mismatch ==="
MISMATCH_TASK=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Mismatch Target"}')
MISMATCH_TASK_ID=$(echo "$MISMATCH_TASK" | jq -r '.id')
# Body has SAVE_TASK_ID but URL targets MISMATCH_TASK_ID
MISMATCH_STATUS=$(api_status -X PUT "$PBASE/tasks/$MISMATCH_TASK_ID/_save" -d "$SAVE_BODY")
assert_eq "400" "$MISMATCH_STATUS" "save with mismatched task ID returns 400"

echo ""
echo "=== Set dependencies (replace all) ==="
DEP_A=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Dep A"}')
DEP_A_ID=$(echo "$DEP_A" | jq -r '.id')
DEP_B=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Dep B"}')
DEP_B_ID=$(echo "$DEP_B" | jq -r '.id')
DEP_C=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Dep C"}')
DEP_C_ID=$(echo "$DEP_C" | jq -r '.id')

# Add initial dep: C depends on A
api_json -X POST "$PBASE/tasks/$DEP_C_ID/deps" -d "{\"dep_id\":$DEP_A_ID}" >/dev/null
DEPS_BEFORE=$(api_get "$PBASE/tasks/$DEP_C_ID/deps")
assert_eq "1" "$(echo "$DEPS_BEFORE" | jq '.items | length')" "set_deps initial: 1 dependency"

# Replace all deps: C now depends on B only
SET_RESULT=$(api_json -X PUT "$PBASE/tasks/$DEP_C_ID/deps" -d "{\"dep_ids\":[$DEP_B_ID]}")
assert_eq "1" "$(echo "$SET_RESULT" | jq '.dependencies | length')" "set_deps: 1 dep after replace"
assert_contains "$(echo "$SET_RESULT" | jq -r '.dependencies[]')" "$DEP_B_ID" "set_deps: dep is B"

# Verify via list endpoint
DEPS_AFTER=$(api_get "$PBASE/tasks/$DEP_C_ID/deps")
assert_eq "1" "$(echo "$DEPS_AFTER" | jq '.items | length')" "set_deps verify: 1 dependency"
assert_json_field "$(echo "$DEPS_AFTER" | jq '.items[0]')" '.id' "$DEP_B_ID" "set_deps verify: dep is B"

# Clear all deps
CLEAR_RESULT=$(api_json -X PUT "$PBASE/tasks/$DEP_C_ID/deps" -d '{"dep_ids":[]}')
assert_eq "0" "$(echo "$CLEAR_RESULT" | jq '.dependencies | length')" "set_deps: cleared all"

echo ""
echo "=== Next when no eligible task ==="
# Complete or cancel remaining active tasks
api_json -X POST "$PBASE/tasks/$TASK4_ID/publish" -d '{}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK4_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK7_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$TASK8_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$PT_TASK_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
api_json -X POST "$PBASE/tasks/$PT_DOD_ID/cancel" -d '{"reason":"cleanup"}' >/dev/null 2>&1 || true
STATUS_NEXT_EMPTY=$(api_status -X POST "$PBASE/tasks/next" -d '{}')
assert_eq "404" "$STATUS_NEXT_EMPTY" "next with no eligible task returns 404"

echo ""
echo "=== Preview next: no candidate ==="
STATUS_PREVIEW_EMPTY=$(api_status "$PBASE/tasks/preview-next")
assert_eq "404" "$STATUS_PREVIEW_EMPTY" "preview-next with no eligible task returns 404"

test_summary
