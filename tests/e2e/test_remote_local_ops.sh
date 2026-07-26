#!/usr/bin/env bash
# E2E tests: Remote/Local mode switching, hook firing, and unblocked_tasks verification.
# Validates that the TaskOperations refactoring (Local/Remote implementations) works correctly.

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

SERVER_PID=""
MASTER_KEY=test-key

start_server() {
  PORT=$(allocate_port)
  API_URL="http://127.0.0.1:$PORT"
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" >/dev/null 2>&1 &
  SERVER_PID=$!
  wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"
  TEST_TOKEN=$(create_test_user_key "$API_URL" "$MASTER_KEY")
}

stop_server() {
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
  fi
}

cleanup_all() {
  stop_server
  cleanup_test_env
}
trap cleanup_all EXIT

run_http() {
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

clear_hook_log() {
  run_lf hooks log --clear >/dev/null 2>&1 || true
}

count_log_entries() {
  local runtime="$1"
  local event="$2"
  local log_file="$XDG_STATE_HOME/senko/hooks.log"
  if [[ ! -f "$log_file" ]]; then
    echo "0"
    return
  fi
  jq -s "[.[] | select(.runtime == \"$runtime\" and .event == \"$event\" and .type == \"event_fired\")] | length" < "$log_file"
}

# ========================================
# Section 1: Local mode (SQLite direct)
# State transitions, DoD, Dependencies
# ========================================
echo "=== Section 1: Local Mode Operations ==="

echo "[1.1] State transitions: draft → todo → in_progress → completed"
T1=$(run_lf task add --title "Local lifecycle" --description "Local mode test")
T1_ID=$(echo "$T1" | jq -r '.id')
assert_json_field "$T1" '.status' "draft" "local: add creates draft"

READY=$(run_lf task publish "$T1_ID")
assert_json_field "$READY" '.status' "todo" "local: ready → todo"

STARTED=$(run_lf task start "$T1_ID")
assert_json_field "$STARTED" '.status' "in_progress" "local: start → in_progress"

COMPLETED=$(run_lf task complete "$T1_ID")
assert_json_field "$COMPLETED" '.status' "completed" "local: complete → completed"

echo "[1.2] Cancel transition"
T2=$(run_lf task add --title "Local cancel")
T2_ID=$(echo "$T2" | jq -r '.id')
run_lf task publish "$T2_ID" >/dev/null
CANCELED=$(run_lf task cancel "$T2_ID" --reason "not needed")
assert_json_field "$CANCELED" '.status' "canceled" "local: cancel works"
assert_json_field "$CANCELED" '.cancel_reason' "not needed" "local: cancel reason"

echo "[1.3] DoD operations"
T3=$(run_lf task add --title "Local DoD" --definition-of-done "[manual] Item 1" --definition-of-done "[manual] Item 2")
T3_ID=$(echo "$T3" | jq -r '.id')
run_lf task publish "$T3_ID" >/dev/null
run_lf task start "$T3_ID" >/dev/null

DOD_CHECK=$(run_lf task dod check "$T3_ID" 1)
assert_eq "true" "$(echo "$DOD_CHECK" | jq '.definition_of_done[0].checked')" "local: dod check"

DOD_UNCHECK=$(run_lf task dod uncheck "$T3_ID" 1)
assert_eq "false" "$(echo "$DOD_UNCHECK" | jq '.definition_of_done[0].checked')" "local: dod uncheck"

# Complete should fail with unchecked DoD
FAIL_COMPLETE=$(run_lf task complete "$T3_ID" 2>&1 || true)
assert_contains "$FAIL_COMPLETE" "unchecked DoD" "local: complete fails with unchecked DoD"

# Check all DoD and complete
run_lf task dod check "$T3_ID" 1 >/dev/null
run_lf task dod check "$T3_ID" 2 >/dev/null
COMPLETED3=$(run_lf task complete "$T3_ID")
assert_json_field "$COMPLETED3" '.status' "completed" "local: complete after all DoD checked"

echo "[1.4] Dependency operations"
T4=$(run_lf task add --title "Local parent")
T4_ID=$(echo "$T4" | jq -r '.id')
T5=$(run_lf task add --title "Local child")
T5_ID=$(echo "$T5" | jq -r '.id')

DEP_ADD=$(run_lf task deps add "$T5_ID" --on "$T4_ID")
assert_contains "$(echo "$DEP_ADD" | jq -r '.dependencies[]')" "$T4_ID" "local: deps add"

DEPS_LIST=$(run_lf task deps list "$T5_ID")
assert_eq "1" "$(echo "$DEPS_LIST" | jq '.items | length')" "local: deps list"

DEP_RM=$(run_lf task deps remove "$T5_ID" --on "$T4_ID")
assert_eq "0" "$(echo "$DEP_RM" | jq '.dependencies | length')" "local: deps remove"

echo "[1.5] Next task: add(assignee=self, DoD) → ready → next → dod check → complete"
export SENKO_USER="default"
T6=$(run_lf task add --title "Local next" --priority p0 --assignee-user-id self --definition-of-done "[manual] Local DoD")
T6_ID=$(echo "$T6" | jq -r '.id')
run_lf task publish "$T6_ID" >/dev/null
NEXT=$(run_lf task next)
assert_json_field "$NEXT" '.status' "in_progress" "local: next auto-starts"
assert_json_field "$NEXT" '.title' "Local next" "local: next picks correct task"
run_lf task dod check "$T6_ID" 1 >/dev/null
run_lf task complete "$T6_ID" >/dev/null
unset SENKO_USER

# ========================================
# Section 2: Remote mode (CLI → API server)
# Same operations via HTTP backend
# ========================================
echo ""
echo "=== Section 2: Remote Mode Operations ==="

# Fresh environment for remote tests
setup_test_env
start_server

echo "[2.1] State transitions via HTTP"
T1=$(run_http task add --title "Remote lifecycle" --description "Remote mode test")
T1_ID=$(echo "$T1" | jq -r '.id')
assert_json_field "$T1" '.status' "draft" "remote: add creates draft"

READY=$(run_http task publish "$T1_ID")
assert_json_field "$READY" '.status' "todo" "remote: ready → todo"

STARTED=$(run_http task start "$T1_ID")
assert_json_field "$STARTED" '.status' "in_progress" "remote: start → in_progress"

COMPLETED=$(run_http task complete "$T1_ID")
assert_json_field "$COMPLETED" '.status' "completed" "remote: complete → completed"

echo "[2.2] Cancel via HTTP"
T2=$(run_http task add --title "Remote cancel")
T2_ID=$(echo "$T2" | jq -r '.id')
run_http task publish "$T2_ID" >/dev/null
CANCELED=$(run_http task cancel "$T2_ID" --reason "http cancel")
assert_json_field "$CANCELED" '.status' "canceled" "remote: cancel works"
assert_json_field "$CANCELED" '.cancel_reason' "http cancel" "remote: cancel reason"

echo "[2.3] DoD operations via HTTP"
T3=$(run_http task add --title "Remote DoD" --definition-of-done "[manual] HTTP item 1" --definition-of-done "[manual] HTTP item 2")
T3_ID=$(echo "$T3" | jq -r '.id')
run_http task publish "$T3_ID" >/dev/null
run_http task start "$T3_ID" >/dev/null

DOD_CHECK=$(run_http task dod check "$T3_ID" 1)
assert_eq "true" "$(echo "$DOD_CHECK" | jq '.definition_of_done[0].checked')" "remote: dod check"

DOD_UNCHECK=$(run_http task dod uncheck "$T3_ID" 1)
assert_eq "false" "$(echo "$DOD_UNCHECK" | jq '.definition_of_done[0].checked')" "remote: dod uncheck"

FAIL_COMPLETE=$(run_http task complete "$T3_ID" 2>&1 || true)
assert_contains "$FAIL_COMPLETE" "unchecked DoD" "remote: complete fails with unchecked DoD"

run_http task dod check "$T3_ID" 1 >/dev/null
run_http task dod check "$T3_ID" 2 >/dev/null
COMPLETED3=$(run_http task complete "$T3_ID")
assert_json_field "$COMPLETED3" '.status' "completed" "remote: complete after all DoD checked"

echo "[2.4] Dependency operations via HTTP"
T4=$(run_http task add --title "Remote parent")
T4_ID=$(echo "$T4" | jq -r '.id')
T5=$(run_http task add --title "Remote child")
T5_ID=$(echo "$T5" | jq -r '.id')

DEP_ADD=$(run_http task deps add "$T5_ID" --on "$T4_ID")
assert_contains "$(echo "$DEP_ADD" | jq -r '.dependencies[]')" "$T4_ID" "remote: deps add"

DEPS_LIST=$(run_http task deps list "$T5_ID")
assert_eq "1" "$(echo "$DEPS_LIST" | jq '.items | length')" "remote: deps list"

DEP_RM=$(run_http task deps remove "$T5_ID" --on "$T4_ID")
assert_eq "0" "$(echo "$DEP_RM" | jq '.dependencies | length')" "remote: deps remove"

echo "[2.5] Next task: add(assignee=self, DoD) → ready → next → dod check → complete"
T6=$(run_http task add --title "Remote next" --priority p0 --assignee-user-id self --definition-of-done "[manual] Remote DoD")
T6_ID=$(echo "$T6" | jq -r '.id')
run_http task publish "$T6_ID" >/dev/null
NEXT=$(run_http task next)
assert_json_field "$NEXT" '.status' "in_progress" "remote: next auto-starts"
assert_json_field "$NEXT" '.title' "Remote next" "remote: next picks correct task"
run_http task dod check "$T6_ID" 1 >/dev/null
run_http task complete "$T6_ID" >/dev/null

stop_server

# ========================================
# Section 3: Remote complete unblocked_tasks
# Verify API response includes unblocked tasks
# ========================================
echo ""
echo "=== Section 3: Remote Complete unblocked_tasks ==="

setup_test_env
start_server

# Get project ID (created by first add)
BLOCKER=$(run_http task add --title "Blocker task")
BLOCKER_ID=$(echo "$BLOCKER" | jq -r '.id')
PROJECT_ID=$(echo "$BLOCKER" | jq -r '.project_id')

BLOCKED=$(run_http task add --title "Blocked task")
BLOCKED_ID=$(echo "$BLOCKED" | jq -r '.id')

# Set up dependency: BLOCKED depends on BLOCKER
run_http task deps add "$BLOCKED_ID" --on "$BLOCKER_ID" >/dev/null

# Move both to todo, start the blocker
run_http task publish "$BLOCKER_ID" >/dev/null
run_http task publish "$BLOCKED_ID" >/dev/null
run_http task start "$BLOCKER_ID" >/dev/null

echo "[3.1] Complete via API returns unblocked_tasks"
# Call the API directly with curl to get the full CompleteTaskResponse
COMPLETE_RESP=$(curl -sf -H "Authorization: Bearer $TEST_TOKEN" -X POST "$API_URL/api/v1/projects/$PROJECT_ID/tasks/$BLOCKER_ID/complete")

assert_json_field "$COMPLETE_RESP" '.task.status' "completed" "api complete: task status"

UNBLOCKED_COUNT=$(echo "$COMPLETE_RESP" | jq '.unblocked_tasks | length')
assert_eq "1" "$UNBLOCKED_COUNT" "api complete: 1 unblocked task"

UNBLOCKED_TITLE=$(echo "$COMPLETE_RESP" | jq -r '.unblocked_tasks[0].title')
assert_eq "Blocked task" "$UNBLOCKED_TITLE" "api complete: unblocked task title"

UNBLOCKED_ID=$(echo "$COMPLETE_RESP" | jq -r '.unblocked_tasks[0].id')
assert_eq "$BLOCKED_ID" "$UNBLOCKED_ID" "api complete: unblocked task id"

echo "[3.2] Complete with no dependencies returns empty unblocked_tasks"
STANDALONE=$(run_http task add --title "Standalone task")
STANDALONE_ID=$(echo "$STANDALONE" | jq -r '.id')
run_http task publish "$STANDALONE_ID" >/dev/null
run_http task start "$STANDALONE_ID" >/dev/null

COMPLETE_RESP2=$(curl -sf -H "Authorization: Bearer $TEST_TOKEN" -X POST "$API_URL/api/v1/projects/$PROJECT_ID/tasks/$STANDALONE_ID/complete")
UNBLOCKED_COUNT2=$(echo "$COMPLETE_RESP2" | jq '.unblocked_tasks | length')
assert_eq "0" "$UNBLOCKED_COUNT2" "api complete: 0 unblocked for standalone task"

stop_server

# Per-runtime hook firing (CLI vs server.remote) is covered in full by
# test_http_hooks.sh under the new hooks schema, so it is no longer duplicated
# here.

test_summary
