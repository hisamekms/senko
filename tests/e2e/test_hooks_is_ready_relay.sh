#!/usr/bin/env bash
# E2E: `is_ready` field in hook payload under relay mode.
#
# Regression test for https://... — relay server fires [server.relay.*] hooks
# from a deserialized Task (whose internal DB id is stripped by serde). The
# `is_task_ready(project_id, task_number)` port contract must use `task_number`
# so relay-side hook payloads carry the correct `is_ready` value.

set -euo pipefail

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

HOOK_LOG="$TEST_DIR/relay-hook.log"

# Configure [server.relay.task_publish.hooks.*] so every task_publish event
# fired on the relay side is appended to the shared log file. Both upstream
# and relay servers share this same config.toml (same --project-root).
mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[server.relay.task_publish.hooks.record]
command = "cat >> $HOOK_LOG"
mode = "sync"
EOF

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

# Run senko CLI through the relay server.
run_relay() {
  SENKO_CLI_REMOTE_URL="$RELAY_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

# Pick the `is_ready` value for the task_publish hook record whose
# payload's event.task.id (task_number) matches the given id.
is_ready_for() {
  local tn="$1"
  jq -r --argjson tn "$tn" \
    'select(.event.event == "task_publish" and .event.task.id == $tn) | .event.is_ready' \
    "$HOOK_LOG" 2>/dev/null | head -1
}

start_servers

echo "--- Section 1: task_publish with no deps (is_ready=true) ---"
T1=$(run_relay task add --title "Relay is_ready no-deps" | jq -r '.id')
run_relay task publish "$T1" >/dev/null
wait_for "hook recorded for T1" 5 \
  "[[ -s '$HOOK_LOG' ]] && [[ \"\$(is_ready_for $T1)\" == 'true' || \"\$(is_ready_for $T1)\" == 'false' ]]"
assert_eq "true" "$(is_ready_for "$T1")" \
  "relay: task_publish (no deps) → is_ready=true"

echo "--- Section 2: task_publish with completed dep (is_ready=true) ---"
# Create a dep task, fully complete it first so it is "completed".
DEP=$(run_relay task add --title "Relay is_ready dep" | jq -r '.id')
run_relay task publish "$DEP" >/dev/null
run_relay task start "$DEP" >/dev/null
run_relay task complete "$DEP" >/dev/null

T2=$(run_relay task add --title "Relay is_ready with completed dep" | jq -r '.id')
run_relay task deps add "$T2" --on "$DEP" >/dev/null
run_relay task publish "$T2" >/dev/null
wait_for "hook recorded for T2" 5 \
  "[[ \"\$(is_ready_for $T2)\" == 'true' || \"\$(is_ready_for $T2)\" == 'false' ]]"
assert_eq "true" "$(is_ready_for "$T2")" \
  "relay: task_publish (completed dep) → is_ready=true"

echo "--- Section 3: task_publish with incomplete dep (is_ready=false) ---"
DEP_OPEN=$(run_relay task add --title "Relay is_ready open dep" | jq -r '.id')
run_relay task publish "$DEP_OPEN" >/dev/null   # todo, not completed

T3=$(run_relay task add --title "Relay is_ready with open dep" | jq -r '.id')
run_relay task deps add "$T3" --on "$DEP_OPEN" >/dev/null
run_relay task publish "$T3" >/dev/null
wait_for "hook recorded for T3" 5 \
  "[[ \"\$(is_ready_for $T3)\" == 'true' || \"\$(is_ready_for $T3)\" == 'false' ]]"
assert_eq "false" "$(is_ready_for "$T3")" \
  "relay: task_publish (incomplete dep) → is_ready=false"

stop_servers

test_summary
