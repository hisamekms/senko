#!/usr/bin/env bash
# E2E tests for remote CLI mode: CLI commands via SENKO_CLI_REMOTE_URL (remote server)
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
API_URL="http://127.0.0.1:$PORT"
MASTER_KEY=test-key

# Start the API server in background
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

# Wait for server to be ready
wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"

# Create a real user and API key (master key is only for user creation)
TEST_TOKEN=$(create_test_user_key "$API_URL" "$MASTER_KEY")

# Helper: run senko CLI in HTTP backend mode
# SENKO_USER is not needed: --assignee-user-id self is resolved by the API from auth token
run_http() {
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

echo "--- Test: HTTP Backend Mode ---"

echo "[1] Add task via HTTP backend"
TASK1=$(run_http task add --title "HTTP Task 1" --description "Created via HTTP backend" --priority p1)
TASK1_ID=$(echo "$TASK1" | jq -r '.id')
assert_json_field "$TASK1" '.title' "HTTP Task 1" "add: title"
assert_json_field "$TASK1" '.status' "draft" "add: status is draft"
assert_json_field "$TASK1" '.priority' "P1" "add: priority"

echo "[2] Get task via HTTP backend"
GOT=$(run_http task get "$TASK1_ID")
assert_json_field "$GOT" '.id' "$TASK1_ID" "get: correct id"
assert_json_field "$GOT" '.title' "HTTP Task 1" "get: correct title"

echo "[3] List tasks via HTTP backend"
LIST=$(run_http task list)
assert_eq "1" "$(echo "$LIST" | jq '.items | length')" "list: 1 task"

echo "[4] Edit task via HTTP backend"
run_http task edit "$TASK1_ID" --title "HTTP Task 1 Updated" --add-tag backend >/dev/null
EDITED=$(run_http task get "$TASK1_ID")
assert_json_field "$EDITED" '.title' "HTTP Task 1 Updated" "edit: title updated"
assert_contains "$(echo "$EDITED" | jq -r '.tags[]')" "backend" "edit: tag added"

echo "[5] Publish task via HTTP backend"
READY=$(run_http task publish "$TASK1_ID")
assert_json_field "$READY" '.status' "todo" "ready: status is todo"

echo "[6] Start task via HTTP backend"
STARTED=$(run_http task start "$TASK1_ID")
assert_json_field "$STARTED" '.status' "in_progress" "start: status is in_progress"

echo "[7] Complete task via HTTP backend"
COMPLETED=$(run_http task complete "$TASK1_ID")
assert_json_field "$COMPLETED" '.status' "completed" "complete: status is completed"

echo "[8] Add task with DoD for DoD check/uncheck test"
TASK2=$(run_http task add --title "DoD Task" --definition-of-done "[manual] Write tests" --definition-of-done "[manual] Deploy")
TASK2_ID=$(echo "$TASK2" | jq -r '.id')
run_http task publish "$TASK2_ID" >/dev/null
run_http task start "$TASK2_ID" >/dev/null

echo "[9] DoD check via HTTP backend"
DOD_CHECKED=$(run_http task dod check "$TASK2_ID" 1)
assert_eq "true" "$(echo "$DOD_CHECKED" | jq '.definition_of_done[0].checked')" "dod check: item 1 checked"

echo "[10] DoD uncheck via HTTP backend"
DOD_UNCHECKED=$(run_http task dod uncheck "$TASK2_ID" 1)
assert_eq "false" "$(echo "$DOD_UNCHECKED" | jq '.definition_of_done[0].checked')" "dod uncheck: item 1 unchecked"

echo "[11] Complete with unchecked DoD should fail"
COMPLETE_FAIL=$(run_http task complete "$TASK2_ID" 2>&1 || true)
assert_contains "$COMPLETE_FAIL" "unchecked DoD" "complete with unchecked DoD fails"

echo "[12] Check all DoD and complete"
run_http task dod check "$TASK2_ID" 1 >/dev/null
run_http task dod check "$TASK2_ID" 2 >/dev/null
COMPLETED2=$(run_http task complete "$TASK2_ID")
assert_json_field "$COMPLETED2" '.status' "completed" "complete after all DoD checked"

echo "[13] Dependencies via HTTP backend"
TASK3=$(run_http task add --title "Dep Parent")
TASK3_ID=$(echo "$TASK3" | jq -r '.id')
TASK4=$(run_http task add --title "Dep Child")
TASK4_ID=$(echo "$TASK4" | jq -r '.id')

DEP_ADDED=$(run_http task deps add "$TASK4_ID" --on "$TASK3_ID")
assert_contains "$(echo "$DEP_ADDED" | jq -r '.dependencies[]')" "$TASK3_ID" "deps add: dependency added"

DEPS_LIST=$(run_http task deps list "$TASK4_ID")
assert_eq "1" "$(echo "$DEPS_LIST" | jq '.items | length')" "deps list: 1 dependency"

DEP_REMOVED=$(run_http task deps remove "$TASK4_ID" --on "$TASK3_ID")
assert_eq "0" "$(echo "$DEP_REMOVED" | jq '.dependencies | length')" "deps remove: dependency removed"

echo "[14] Cancel task via HTTP backend"
run_http task publish "$TASK3_ID" >/dev/null
CANCELED=$(run_http task cancel "$TASK3_ID" --reason "not needed")
assert_json_field "$CANCELED" '.status' "canceled" "cancel: status is canceled"
assert_json_field "$CANCELED" '.cancel_reason' "not needed" "cancel: reason set"

echo "[15] Next task: add(assignee=self, DoD) → ready → next → dod check → complete"
TASK5=$(run_http task add --title "Next Candidate" --priority p0 --assignee-user-id self --definition-of-done "[manual] Next DoD")
TASK5_ID=$(echo "$TASK5" | jq -r '.id')
run_http task publish "$TASK5_ID" >/dev/null
NEXT=$(run_http task next)
assert_json_field "$NEXT" '.status' "in_progress" "next: auto-starts task"
assert_json_field "$NEXT" '.title' "Next Candidate" "next: picks correct task"
run_http task dod check "$TASK5_ID" 1 >/dev/null
run_http task complete "$TASK5_ID" >/dev/null

echo "[16] Config via HTTP backend"
CONFIG=$(run_http config)
assert_json_field "$CONFIG" '.workflow.merge_via' "direct" "config: merge_via"

echo "[17] List with filters via HTTP backend"
LIST_COMPLETED=$(run_http task list --status completed)
COMPLETED_COUNT=$(echo "$LIST_COMPLETED" | jq '.items | length')
assert_eq "3" "$COMPLETED_COUNT" "list filter: 3 completed tasks"

test_summary
