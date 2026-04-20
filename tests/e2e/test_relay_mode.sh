#!/usr/bin/env bash
# E2E tests for relay mode: CLI → Relay Server (HTTP backend) → Upstream Server (SQLite)
# Verifies that all API endpoints work correctly through a 3-layer relay chain,
# and that hooks fire properly.

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

UPSTREAM_PID=""
RELAY_PID=""
MASTER_KEY=test-key

UPSTREAM_PORT=$(allocate_port 0)
RELAY_PORT=$(allocate_port 1)
UPSTREAM_URL="http://127.0.0.1:$UPSTREAM_PORT"
RELAY_URL="http://127.0.0.1:$RELAY_PORT"

start_servers() {
  # Start upstream server (SQLite backend)
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
    serve --port "$UPSTREAM_PORT" >/dev/null 2>&1 &
  UPSTREAM_PID=$!

  wait_for "upstream server ready" 10 "curl -sf $UPSTREAM_URL/api/v1/health >/dev/null"

  # Start relay server (HTTP backend forwarding to upstream)
  SENKO_SERVER_RELAY_URL="$UPSTREAM_URL" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    serve --port "$RELAY_PORT" >/dev/null 2>&1 &
  RELAY_PID=$!

  wait_for "relay server ready" 10 "curl -sf $RELAY_URL/api/v1/health >/dev/null"

  # Create test user and API key on upstream
  TEST_TOKEN=$(create_test_user_key "$UPSTREAM_URL" "$MASTER_KEY")
}

stop_servers() {
  if [[ -n "$RELAY_PID" ]]; then
    kill "$RELAY_PID" 2>/dev/null || true
    wait "$RELAY_PID" 2>/dev/null || true
    RELAY_PID=""
  fi
  if [[ -n "$UPSTREAM_PID" ]]; then
    kill "$UPSTREAM_PID" 2>/dev/null || true
    wait "$UPSTREAM_PID" 2>/dev/null || true
    UPSTREAM_PID=""
  fi
}

cleanup_all() {
  stop_servers
  cleanup_test_env
}
trap cleanup_all EXIT

# Helper: run senko CLI through the relay server
run_relay() {
  SENKO_CLI_REMOTE_URL="$RELAY_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

# Helper: API call through the relay server
relay_api_status() {
  curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

assert_gte() {
  local actual="$1"
  local threshold="$2"
  local message="$3"
  if [[ "$actual" -ge "$threshold" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message"
    echo "    expected: >= $threshold"
    echo "    actual:   $actual"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

start_servers

# ========================================
# Section 1: Task CRUD (DoD #1)
# ========================================
echo "--- Section 1: Task CRUD via relay ---"

echo "[1.1] Create task"
TASK1=$(run_relay task add --title "Relay Task 1" --description "Created via relay" --priority p1)
TASK1_ID=$(echo "$TASK1" | jq -r '.id')
assert_json_field "$TASK1" '.title' "Relay Task 1" "create: title"
assert_json_field "$TASK1" '.status' "draft" "create: status is draft"
assert_json_field "$TASK1" '.priority' "P1" "create: priority"
assert_json_field "$TASK1" '.description' "Created via relay" "create: description"

echo "[1.2] Get task"
GOT=$(run_relay task get "$TASK1_ID")
assert_json_field "$GOT" '.id' "$TASK1_ID" "get: correct id"
assert_json_field "$GOT" '.title' "Relay Task 1" "get: correct title"

echo "[1.3] Edit task"
run_relay task edit "$TASK1_ID" --title "Relay Task 1 Updated" --add-tag relay >/dev/null
EDITED=$(run_relay task get "$TASK1_ID")
assert_json_field "$EDITED" '.title' "Relay Task 1 Updated" "edit: title updated"
assert_contains "$(echo "$EDITED" | jq -r '.tags[]')" "relay" "edit: tag added"

echo "[1.4] Delete task (via API through relay)"
DEL_STATUS=$(relay_api_status -X DELETE "$RELAY_URL/api/v1/projects/1/tasks/$TASK1_ID")
assert_eq "204" "$DEL_STATUS" "delete: returns 204"

echo "[1.5] Verify deleted task is gone"
LIST_AFTER_DEL=$(run_relay task list)
REMAINING=$(echo "$LIST_AFTER_DEL" | jq "[.[] | select(.id == $TASK1_ID)] | length")
assert_eq "0" "$REMAINING" "delete: task no longer in list"

# ========================================
# Section 2: Status transitions (DoD #2)
# ========================================
echo "--- Section 2: Status transitions via relay ---"

echo "[2.1] Create and ready task"
TASK2=$(run_relay task add --title "Transition Task")
TASK2_ID=$(echo "$TASK2" | jq -r '.id')
READY=$(run_relay task publish "$TASK2_ID")
assert_json_field "$READY" '.status' "todo" "ready: status is todo"

echo "[2.2] Start task"
STARTED=$(run_relay task start "$TASK2_ID")
assert_json_field "$STARTED" '.status' "in_progress" "start: status is in_progress"

echo "[2.3] Complete task"
COMPLETED=$(run_relay task complete "$TASK2_ID")
assert_json_field "$COMPLETED" '.status' "completed" "complete: status is completed"

echo "[2.4] Create, ready, and cancel task"
TASK3=$(run_relay task add --title "Cancel Task")
TASK3_ID=$(echo "$TASK3" | jq -r '.id')
run_relay task publish "$TASK3_ID" >/dev/null
CANCELED=$(run_relay task cancel "$TASK3_ID" --reason "not needed")
assert_json_field "$CANCELED" '.status' "canceled" "cancel: status is canceled"
assert_json_field "$CANCELED" '.cancel_reason' "not needed" "cancel: reason set"

echo "[2.5] Next task: add(assignee=self, DoD) → ready → next → dod check → complete"
TASK4=$(run_relay task add --title "Next Candidate" --priority p0 --assignee-user-id self --definition-of-done "Next DoD")
TASK4_ID=$(echo "$TASK4" | jq -r '.id')
run_relay task publish "$TASK4_ID" >/dev/null
NEXT=$(run_relay task next)
assert_json_field "$NEXT" '.status' "in_progress" "next: auto-starts task"
assert_json_field "$NEXT" '.title' "Next Candidate" "next: picks correct task"
run_relay task dod check "$TASK4_ID" 1 >/dev/null
run_relay task complete "$TASK4_ID" >/dev/null

# ========================================
# Section 3: Dependencies (DoD #3)
# ========================================
echo "--- Section 3: Dependencies via relay ---"

PARENT=$(run_relay task add --title "Dep Parent")
PARENT_ID=$(echo "$PARENT" | jq -r '.id')
CHILD=$(run_relay task add --title "Dep Child")
CHILD_ID=$(echo "$CHILD" | jq -r '.id')

echo "[3.1] Add dependency"
DEP_ADDED=$(run_relay task deps add "$CHILD_ID" --on "$PARENT_ID")
assert_contains "$(echo "$DEP_ADDED" | jq -r '.dependencies[]')" "$PARENT_ID" "deps add: dependency added"

echo "[3.2] List dependencies"
DEPS_LIST=$(run_relay task deps list "$CHILD_ID")
assert_eq "1" "$(echo "$DEPS_LIST" | jq 'length')" "deps list: 1 dependency"

echo "[3.3] Remove dependency"
DEP_REMOVED=$(run_relay task deps remove "$CHILD_ID" --on "$PARENT_ID")
assert_eq "0" "$(echo "$DEP_REMOVED" | jq '.dependencies | length')" "deps remove: dependency removed"

# ========================================
# Section 4: DoD operations (DoD #4)
# ========================================
echo "--- Section 4: DoD operations via relay ---"

TASK5=$(run_relay task add --title "DoD Task" --definition-of-done "Write tests" --definition-of-done "Deploy")
TASK5_ID=$(echo "$TASK5" | jq -r '.id')
run_relay task publish "$TASK5_ID" >/dev/null
run_relay task start "$TASK5_ID" >/dev/null

echo "[4.1] DoD check"
DOD_CHECKED=$(run_relay task dod check "$TASK5_ID" 1)
assert_eq "true" "$(echo "$DOD_CHECKED" | jq '.definition_of_done[0].checked')" "dod check: item 1 checked"

echo "[4.2] DoD uncheck"
DOD_UNCHECKED=$(run_relay task dod uncheck "$TASK5_ID" 1)
assert_eq "false" "$(echo "$DOD_UNCHECKED" | jq '.definition_of_done[0].checked')" "dod uncheck: item 1 unchecked"

echo "[4.3] Complete with unchecked DoD should fail"
COMPLETE_FAIL=$(run_relay task complete "$TASK5_ID" 2>&1 || true)
assert_contains "$COMPLETE_FAIL" "unchecked DoD" "complete with unchecked DoD fails"

echo "[4.4] Check all DoD and complete"
run_relay task dod check "$TASK5_ID" 1 >/dev/null
run_relay task dod check "$TASK5_ID" 2 >/dev/null
COMPLETED5=$(run_relay task complete "$TASK5_ID")
assert_json_field "$COMPLETED5" '.status' "completed" "complete after all DoD checked"

# ========================================
# Section 5: Metadata fields (DoD #5)
# ========================================
echo "--- Section 5: Metadata fields via relay ---"

echo "[5.1] Add metadata field"
MF_ADD=$(run_relay project metadata-field add --name sprint --type string --description "Sprint name")
assert_json_field "$MF_ADD" '.name' "sprint" "metadata-field add: name"
assert_json_field "$MF_ADD" '.field_type' "string" "metadata-field add: type"

echo "[5.2] Add second metadata field"
MF_ADD2=$(run_relay project metadata-field add --name points --type number --required-on-complete)
assert_json_field "$MF_ADD2" '.name' "points" "metadata-field add: second field"

echo "[5.3] List metadata fields"
MF_LIST=$(run_relay project metadata-field list)
assert_eq "2" "$(echo "$MF_LIST" | jq 'length')" "metadata-field list: 2 fields"

echo "[5.4] Remove metadata field"
MF_REMOVE=$(run_relay project metadata-field remove --name sprint)
assert_json_field "$MF_REMOVE" '.deleted' "sprint" "metadata-field remove: deleted"

echo "[5.5] List after removal"
MF_LIST2=$(run_relay project metadata-field list)
assert_eq "1" "$(echo "$MF_LIST2" | jq 'length')" "metadata-field list: 1 field after removal"

# Clean up remaining metadata field so it does not block task completion in later sections
run_relay project metadata-field remove --name points >/dev/null

# ========================================
# Section 7: Metadata happy path via relay (regression)
# ========================================
echo "--- Section 7: Metadata happy path via relay ---"

echo "[7.1] Create task with DoD and assignee=self"
META_TASK=$(run_relay task add --title "Metadata Relay Task" \
  --assignee-user-id self \
  --definition-of-done "Write tests" --definition-of-done "Review code")
META_TASK_ID=$(echo "$META_TASK" | jq -r '.id')
assert_json_field "$META_TASK" '.status' "draft" "meta: created as draft"

echo "[7.2] Publish task"
run_relay task publish "$META_TASK_ID" >/dev/null

echo "[7.3] Start via next --metadata"
NEXT_META=$(run_relay task next --metadata '{"sprint":"v1","points":5}')
assert_json_field "$NEXT_META" '.status' "in_progress" "meta: next starts task"
assert_json_field "$NEXT_META" '.metadata.sprint' "v1" "meta: metadata.sprint set via next"
assert_json_field "$NEXT_META" '.metadata.points' "5" "meta: metadata.points set via next"

echo "[7.4] Verify metadata persisted (get)"
META_GOT=$(run_relay task get "$META_TASK_ID")
assert_json_field "$META_GOT" '.metadata.sprint' "v1" "meta: metadata.sprint persisted"
assert_json_field "$META_GOT" '.metadata.points' "5" "meta: metadata.points persisted"

echo "[7.5] Check DoD items"
run_relay task dod check "$META_TASK_ID" 1 >/dev/null
run_relay task dod check "$META_TASK_ID" 2 >/dev/null

echo "[7.6] Complete task"
META_COMPLETED=$(run_relay task complete "$META_TASK_ID")
assert_json_field "$META_COMPLETED" '.status' "completed" "meta: task completed"
assert_json_field "$META_COMPLETED" '.metadata.sprint' "v1" "meta: metadata survives complete"

# ========================================
# Section 6: Hook firing via relay (DoD #6)
# ========================================
echo "--- Section 6: Hook firing via relay ---"

# Stop servers to reconfigure with hooks
stop_servers

# Write hook config
mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[cli.task_publish.hooks.test_hook]
command = "true"
enabled = true

[cli.task_start.hooks.test_hook]
command = "true"
enabled = true

[cli.task_complete.hooks.test_hook]
command = "true"
enabled = true

[cli.task_cancel.hooks.test_hook]
command = "true"
enabled = true
EOF

UPSTREAM_PORT=$(allocate_port 0)
RELAY_PORT=$(allocate_port 1)
UPSTREAM_URL="http://127.0.0.1:$UPSTREAM_PORT"
RELAY_URL="http://127.0.0.1:$RELAY_PORT"

# Restart servers with hook config
start_servers

# Clear any existing hook log
run_lf hooks log --clear >/dev/null 2>&1 || true

# Run transitions through relay
HOOK_T1=$(run_relay task add --title "Hook Task 1")
HOOK_T1_ID=$(echo "$HOOK_T1" | jq -r '.id')
run_relay task publish "$HOOK_T1_ID" >/dev/null
run_relay task start "$HOOK_T1_ID" >/dev/null
run_relay task complete "$HOOK_T1_ID" >/dev/null

HOOK_T2=$(run_relay task add --title "Hook Task 2")
HOOK_T2_ID=$(echo "$HOOK_T2" | jq -r '.id')
run_relay task publish "$HOOK_T2_ID" >/dev/null
run_relay task cancel "$HOOK_T2_ID" --reason "test cancel" >/dev/null

sleep 1

HOOK_LOG="$XDG_STATE_HOME/senko/hooks.log"

count_log_entries() {
  local runtime="$1"
  local event="$2"
  if [[ ! -f "$HOOK_LOG" ]]; then
    echo "0"
    return
  fi
  jq -s "[.[] | select(.runtime == \"$runtime\" and .event == \"$event\" and .type == \"event_fired\")] | length" < "$HOOK_LOG"
}

echo "[6.1] CLI fires task_publish via relay"
assert_gte "$(count_log_entries cli task_publish)" 1 "relay: cli fires task_publish"

echo "[6.2] CLI fires task_start via relay"
assert_gte "$(count_log_entries cli task_start)" 1 "relay: cli fires task_start"

echo "[6.3] CLI fires task_complete via relay"
assert_gte "$(count_log_entries cli task_complete)" 1 "relay: cli fires task_complete"

echo "[6.4] CLI fires task_cancel via relay"
assert_gte "$(count_log_entries cli task_cancel)" 1 "relay: cli fires task_cancel"

test_summary
