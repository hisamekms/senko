#!/usr/bin/env bash
# E2E test: remote mode (CLI → relay → upstream) must not force a master-gated
# user lookup for `task start` / `task next` / `task list --ready` when
# config.user.name is set on the CLI side.
#
# Repro of senko task #330:
#   CLI pre-resolves config.user.name via GET /api/v1/users which is
#   require_master, so any non-master API key gets 403.
#
# This file ships with the GREEN (post-fix) assertions. A RED-phase proof
# that the bug actually reproduces pre-fix lives in test_remote_user_resolve_red.sh
# and is expected to pass ONLY against the unfixed code. That companion test
# is a one-shot reproduction artifact.

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
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
    serve --port "$UPSTREAM_PORT" >/dev/null 2>&1 &
  UPSTREAM_PID=$!
  wait_for "upstream server ready" 10 "curl -sf $UPSTREAM_URL/api/v1/health >/dev/null"

  SENKO_SERVER_RELAY_URL="$UPSTREAM_URL" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    serve --port "$RELAY_PORT" >/dev/null 2>&1 &
  RELAY_PID=$!
  wait_for "relay server ready" 10 "curl -sf $RELAY_URL/api/v1/health >/dev/null"
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

start_servers

# --- Create a non-master user + API key on upstream ---
USERNAME="remote_user_resolve_$$"
USER_JSON=$(curl -sf -X POST "$UPSTREAM_URL/api/v1/users" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $MASTER_KEY" \
  -d "{\"username\":\"$USERNAME\"}")
USER_ID=$(echo "$USER_JSON" | jq -r '.id')

# Grant project access (via local CLI, bypassing API auth)
run_lf project members add --user-id "$USER_ID" --role owner >/dev/null

KEY_JSON=$(curl -sf -X POST "$UPSTREAM_URL/api/v1/users/$USER_ID/api-keys" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $MASTER_KEY" \
  -d '{}')
TEST_TOKEN=$(echo "$KEY_JSON" | jq -r '.key')

# --- Seed two todo tasks assigned to the test user (via local DB write) ---
SEED_TASK=$(run_lf --output json task add \
  --title "Seed for remote user resolve test" \
  --assignee-user-id "$USER_ID" \
  --priority p1)
SEED_TASK_ID=$(echo "$SEED_TASK" | jq -r '.id')
run_lf task publish "$SEED_TASK_ID" >/dev/null

SEED_TASK_2=$(run_lf --output json task add \
  --title "Seed #2 for remote user resolve test" \
  --assignee-user-id "$USER_ID" \
  --priority p2)
SEED_TASK_2_ID=$(echo "$SEED_TASK_2" | jq -r '.id')
run_lf task publish "$SEED_TASK_2_ID" >/dev/null

# --- Configure CLI with config.user.name matching the non-master user ---
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[user]
name = "$USERNAME"
EOF

# Helper: run CLI in remote mode as the non-master user through the relay
run_as_user() {
  SENKO_CLI_REMOTE_URL="$RELAY_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

echo "=== Section 1: task list --ready must not require master ==="
LIST_OUT=$(run_as_user task list --ready 2>&1 || true)
assert_not_contains "$LIST_OUT" "master privilege required" "list --ready: no master-privilege error"
LIST_IDS=$(echo "$LIST_OUT" | jq -r '.[].id' 2>/dev/null || echo "")
if echo "$LIST_IDS" | grep -qx "$SEED_TASK_ID"; then
  echo "  PASS: list --ready returns seed task"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: list --ready did not return seed task"
  echo "    output: $LIST_OUT"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "=== Section 2: task next must not require master ==="
NEXT_OUT=$(run_as_user task next 2>&1 || true)
assert_not_contains "$NEXT_OUT" "master privilege required" "next: no master-privilege error"
NEXT_ID=$(echo "$NEXT_OUT" | jq -r '.id' 2>/dev/null || echo "")
assert_eq "$SEED_TASK_ID" "$NEXT_ID" "next: returns highest-priority todo task (id $SEED_TASK_ID)"
NEXT_STATUS=$(echo "$NEXT_OUT" | jq -r '.status' 2>/dev/null || echo "")
assert_eq "in_progress" "$NEXT_STATUS" "next: task transitioned to in_progress"

echo "=== Section 3: task start must not require master ==="
# SEED_TASK_2_ID is still todo
START_OUT=$(run_as_user task start "$SEED_TASK_2_ID" 2>&1 || true)
assert_not_contains "$START_OUT" "master privilege required" "start: no master-privilege error"
START_STATUS=$(echo "$START_OUT" | jq -r '.status' 2>/dev/null || echo "")
assert_eq "in_progress" "$START_STATUS" "start: task moved to in_progress"

test_summary
