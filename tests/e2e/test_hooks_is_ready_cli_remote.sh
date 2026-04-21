#!/usr/bin/env bash
# E2E: `is_ready` field in hook payload under CLI remote mode.
#
# When the CLI is configured with SENKO_CLI_REMOTE_URL, [cli.*] hooks fire
# locally in the CLI process. The Task driving the hook payload was
# deserialized from HTTP, so its internal DB id is 0 — the port must resolve
# readiness by `task_number` instead.

set -euo pipefail

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

SERVER_PID=""
MASTER_KEY=test-key

PORT=$(allocate_port)
API_URL="http://127.0.0.1:$PORT"

HOOK_LOG="$TEST_DIR/cli-hook.log"

# Configure [cli.task_publish.hooks.*] to record every hook payload fired by
# the CLI process itself.
mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_publish.hooks.record]
command = "cat >> $HOOK_LOG"
mode = "sync"
EOF

start_server() {
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
    --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
    serve --port "$PORT" >/dev/null 2>&1 &
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

# Run senko CLI in remote mode (talks to the local server via HTTP).
run_remote() {
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

is_ready_for() {
  local tn="$1"
  jq -r --argjson tn "$tn" \
    'select(.event.event == "task_publish" and .event.task.id == $tn) | .event.is_ready' \
    "$HOOK_LOG" 2>/dev/null | head -1
}

start_server

echo "--- Section 1: task_publish with no deps (is_ready=true) ---"
T1=$(run_remote task add --title "CLI remote is_ready no-deps" | jq -r '.id')
run_remote task publish "$T1" >/dev/null
wait_for "hook recorded for T1" 5 \
  "[[ -s '$HOOK_LOG' ]] && [[ \"\$(is_ready_for $T1)\" == 'true' || \"\$(is_ready_for $T1)\" == 'false' ]]"
assert_eq "true" "$(is_ready_for "$T1")" \
  "cli remote: task_publish (no deps) → is_ready=true"

echo "--- Section 2: task_publish with completed dep (is_ready=true) ---"
DEP=$(run_remote task add --title "CLI remote is_ready dep" | jq -r '.id')
run_remote task publish "$DEP" >/dev/null
run_remote task start "$DEP" >/dev/null
run_remote task complete "$DEP" >/dev/null

T2=$(run_remote task add --title "CLI remote is_ready with completed dep" | jq -r '.id')
run_remote task deps add "$T2" --on "$DEP" >/dev/null
run_remote task publish "$T2" >/dev/null
wait_for "hook recorded for T2" 5 \
  "[[ \"\$(is_ready_for $T2)\" == 'true' || \"\$(is_ready_for $T2)\" == 'false' ]]"
assert_eq "true" "$(is_ready_for "$T2")" \
  "cli remote: task_publish (completed dep) → is_ready=true"

echo "--- Section 3: task_publish with incomplete dep (is_ready=false) ---"
DEP_OPEN=$(run_remote task add --title "CLI remote is_ready open dep" | jq -r '.id')
run_remote task publish "$DEP_OPEN" >/dev/null

T3=$(run_remote task add --title "CLI remote is_ready with open dep" | jq -r '.id')
run_remote task deps add "$T3" --on "$DEP_OPEN" >/dev/null
run_remote task publish "$T3" >/dev/null
wait_for "hook recorded for T3" 5 \
  "[[ \"\$(is_ready_for $T3)\" == 'true' || \"\$(is_ready_for $T3)\" == 'false' ]]"
assert_eq "false" "$(is_ready_for "$T3")" \
  "cli remote: task_publish (incomplete dep) → is_ready=false"

stop_server

test_summary
