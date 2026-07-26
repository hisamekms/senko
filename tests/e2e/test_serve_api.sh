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
TASK2=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Task Two","priority":"P1","tags":["backend"],"definition_of_done":[{"content":"Write tests","verification_type":"manual"},{"content":"Deploy","verification_type":"manual"}]}')
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
TASK5=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Next Candidate","priority":"P0","assignee_user_id":"self","definition_of_done":[{"content":"Next DoD","verification_type":"manual"}]}')
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
PT_DOD=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Preview DoD","definition_of_done":[{"content":"Check me","verification_type":"manual"}]}')
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

# ---------------------------------------------------------------------------
# Sort + composite cursor coverage (Task #430)
#
# We use a fresh project so the rows we create have known relative ages and
# priorities, independent of the test churn above.
# ---------------------------------------------------------------------------

echo ""
echo "=== Sort: setup project ==="
SORT_PROJECT=$(api_json -X POST "$BASE/projects" -d '{"name":"sort-test"}')
SORT_PID=$(echo "$SORT_PROJECT" | jq -r '.id')
SBASE="$BASE/projects/$SORT_PID"

# Create tasks with varying priorities. Each create is a separate request so
# created_at / updated_at advance monotonically — the *last* created row will
# have the newest updated_at.
SORT_T1=$(api_json -X POST "$SBASE/tasks" -d '{"title":"sort-a","priority":"P3"}' | jq -r '.id')
SORT_T2=$(api_json -X POST "$SBASE/tasks" -d '{"title":"sort-b","priority":"P0"}' | jq -r '.id')
SORT_T3=$(api_json -X POST "$SBASE/tasks" -d '{"title":"sort-c","priority":"P2"}' | jq -r '.id')

echo ""
echo "=== Sort: order_by=updated_at desc ==="
SORT_RECENT=$(api_get "$SBASE/tasks?order_by=updated_at&order=desc")
# updated_at desc → most recently created task first (sort-c, then sort-b, sort-a).
assert_eq "sort-c" "$(echo "$SORT_RECENT" | jq -r '.items[0].title')" "updated_at desc: newest first"
assert_eq "sort-b" "$(echo "$SORT_RECENT" | jq -r '.items[1].title')" "updated_at desc: middle"
assert_eq "sort-a" "$(echo "$SORT_RECENT" | jq -r '.items[2].title')" "updated_at desc: oldest last"

echo ""
echo "=== Sort: order_by=priority asc puts P0 first ==="
SORT_PRIO=$(api_get "$SBASE/tasks?order_by=priority&order=asc")
assert_eq "sort-b" "$(echo "$SORT_PRIO" | jq -r '.items[0].title')" "priority asc: P0 first"
assert_eq "sort-c" "$(echo "$SORT_PRIO" | jq -r '.items[1].title')" "priority asc: P2 second"
assert_eq "sort-a" "$(echo "$SORT_PRIO" | jq -r '.items[2].title')" "priority asc: P3 last"

echo ""
echo "=== Sort: composite cursor pagination across 3 pages ==="
PAGE1=$(api_get "$SBASE/tasks?order_by=updated_at&order=desc&limit=1")
assert_eq "sort-c" "$(echo "$PAGE1" | jq -r '.items[0].title')" "page1 item is sort-c"
P1_CURSOR=$(echo "$PAGE1" | jq -r '.next_cursor')
[ "$P1_CURSOR" != "null" ] || { echo "FAIL: page1 next_cursor null"; exit 1; }
# Cursor must be base64 of {"k":"updated_at","v":...,"id":...}.
# next_cursor is unpadded base64url; restore standard alphabet + padding so
# both BSD and GNU base64 -d decode the full payload.
P1_B64=$(printf '%s' "$P1_CURSOR" | tr '_-' '/+')
case $(( ${#P1_B64} % 4 )) in
  2) P1_B64="${P1_B64}==" ;;
  3) P1_B64="${P1_B64}=" ;;
esac
DECODED=$(printf '%s' "$P1_B64" | base64 -d 2>/dev/null || true)
echo "$DECODED" | jq -e '.k == "updated_at" and (.id | type == "number") and (.v | type == "string")' >/dev/null \
  || { echo "FAIL: composite cursor shape unexpected: $DECODED"; exit 1; }
echo "PASS: composite cursor has shape {k:updated_at,v,id}"

PAGE2=$(api_get "$SBASE/tasks?order_by=updated_at&order=desc&limit=1&after=$P1_CURSOR")
assert_eq "sort-b" "$(echo "$PAGE2" | jq -r '.items[0].title')" "page2 item is sort-b"
P2_CURSOR=$(echo "$PAGE2" | jq -r '.next_cursor')
PAGE3=$(api_get "$SBASE/tasks?order_by=updated_at&order=desc&limit=1&after=$P2_CURSOR")
assert_eq "sort-a" "$(echo "$PAGE3" | jq -r '.items[0].title')" "page3 item is sort-a"
assert_eq "null" "$(echo "$PAGE3" | jq -r '.next_cursor')" "page3 has no next_cursor"

echo ""
echo "=== Sort: cursor mismatch returns 400 ==="
# Build an id-only cursor: base64({"id": 1}).
ID_ONLY_CURSOR=$(printf '{"id":1}' | base64 | tr '+/' '-_' | tr -d '=')
STATUS_MISMATCH=$(api_status "$SBASE/tasks?order_by=updated_at&after=$ID_ONLY_CURSOR")
assert_eq "400" "$STATUS_MISMATCH" "id-only cursor with order_by=updated_at returns 400"

echo ""
echo "=== Sort: invalid order_by value returns 400 ==="
STATUS_BAD_ORDER_BY=$(api_status "$SBASE/tasks?order_by=garbage")
assert_eq "400" "$STATUS_BAD_ORDER_BY" "unknown order_by returns 400"

echo ""
echo "=== Sort: invalid order direction returns 400 ==="
STATUS_BAD_ORDER=$(api_status "$SBASE/tasks?order=ascending")
assert_eq "400" "$STATUS_BAD_ORDER" "unknown order returns 400"

echo ""
echo "=== Sort: contracts order_by=updated_at desc ==="
api_json -X POST "$SBASE/contracts" -d '{"title":"contract-a"}' >/dev/null
api_json -X POST "$SBASE/contracts" -d '{"title":"contract-b"}' >/dev/null
api_json -X POST "$SBASE/contracts" -d '{"title":"contract-c"}' >/dev/null
CONTRACTS_RECENT=$(api_get "$SBASE/contracts?order_by=updated_at&order=desc")
assert_eq "contract-c" "$(echo "$CONTRACTS_RECENT" | jq -r '.items[0].title')" "contracts updated_at desc: newest first"
assert_eq "contract-a" "$(echo "$CONTRACTS_RECENT" | jq -r '.items[2].title')" "contracts updated_at desc: oldest last"

echo ""
echo "=== Sort: contracts order_by=priority returns 400 ==="
STATUS_CONTRACTS_PRIORITY=$(api_status "$SBASE/contracts?order_by=priority")
assert_eq "400" "$STATUS_CONTRACTS_PRIORITY" "contracts reject order_by=priority"

echo ""
echo "=== Sort: legacy id-only cursor still works for default order_by=id ==="
ID_PAGE1=$(api_get "$SBASE/tasks?limit=1")
ID_CURSOR=$(echo "$ID_PAGE1" | jq -r '.next_cursor')
ID_PAGE2=$(api_get "$SBASE/tasks?limit=1&after=$ID_CURSOR")
# Default order_by=id ASC; second page should be sort-b (id 2) then sort-c (id 3).
assert_eq "sort-b" "$(echo "$ID_PAGE2" | jq -r '.items[0].title')" "default-order pagination still works"

test_summary
