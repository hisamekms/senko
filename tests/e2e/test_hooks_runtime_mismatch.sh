#!/usr/bin/env bash
# e2e test: runtime-mismatch warning and load-time validation warnings.
#
# These warnings rely on tracing being initialized. The CLI binary emits them
# inside `senko serve`, which calls `init_tracing` at startup, so we verify the
# warnings via the server's startup log stream.
#
# Covered warnings:
#   - hooks defined under a runtime section that does not match the active
#     runtime (e.g. [cli.*] in a server.remote process)
#   - pre+async+abort hooks (abort is unreachable; warning during load)
#   - on_result on non-task_select hooks (ignored; warning during load)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: hooks runtime-mismatch / load-time warnings ---"

mkdir -p "$TEST_PROJECT_ROOT/.senko"

MASTER_KEY=test-key

start_server_capture() {
  local log_file="$1"
  PORT=$(allocate_port)
  API_URL="http://127.0.0.1:$PORT"
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" >"$log_file" 2>&1 &
  SERVER_PID=$!
  wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"
}

stop_server() {
  if [[ -n "${SERVER_PID:-}" ]]; then
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

# ---------------------------------------------------------------
# Case 1: server.remote runtime + hook defined under [cli.*]
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.task_add.hooks.stray]
command = "true"
TOML

LOG1="$TEST_DIR/server1.log"
start_server_capture "$LOG1"
stop_server
LOG1_CONTENT="$(cat "$LOG1")"

echo "[1] server.remote runtime warns about foreign [cli.*] hooks"
assert_contains "$LOG1_CONTENT" "do not match the active runtime" "mismatch warning message present"
assert_contains "$LOG1_CONTENT" "cli" "mismatch warning mentions cli"

# ---------------------------------------------------------------
# Case 2: server.remote runtime + hook defined under [server.relay.*]
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.relay.task_add.hooks.stray]
command = "true"
TOML

LOG2="$TEST_DIR/server2.log"
start_server_capture "$LOG2"
stop_server
LOG2_CONTENT="$(cat "$LOG2")"

echo "[2] server.remote runtime warns about foreign [server.relay.*] hooks"
assert_contains "$LOG2_CONTENT" "do not match the active runtime" "mismatch warning message present"
assert_contains "$LOG2_CONTENT" "server.relay" "mismatch warning mentions server.relay"

# ---------------------------------------------------------------
# Case 3: matching [server.remote.*] hook only — no mismatch warning
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.remote.task_add.hooks.ok]
command = "true"
TOML

LOG3="$TEST_DIR/server3.log"
start_server_capture "$LOG3"
stop_server
LOG3_CONTENT="$(cat "$LOG3")"

echo "[3] matching [server.remote.*] hook emits no mismatch warning"
assert_not_contains "$LOG3_CONTENT" "do not match the active runtime" "no mismatch warning for matching runtime"

# ---------------------------------------------------------------
# Case 4: pre+async+abort is unreachable — load-time warning
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.remote.task_publish.hooks.unreachable_abort]
command = "true"
when = "pre"
mode = "async"
on_failure = "abort"
TOML

LOG4="$TEST_DIR/server4.log"
start_server_capture "$LOG4"
stop_server
LOG4_CONTENT="$(cat "$LOG4")"

echo "[4] load-time warning for pre+async+abort hook"
assert_contains "$LOG4_CONTENT" "pre+async hooks cannot abort" "pre+async+abort warning present"

# ---------------------------------------------------------------
# Case 5: on_result on non-task_select hook — load-time warning
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.remote.task_add.hooks.bad_on_result]
command = "true"
on_result = "selected"
TOML

LOG5="$TEST_DIR/server5.log"
start_server_capture "$LOG5"
stop_server
LOG5_CONTENT="$(cat "$LOG5")"

echo "[5] load-time warning for on_result on non-task_select hook"
assert_contains "$LOG5_CONTENT" "on_result is only meaningful for task_select" "on_result-misuse warning present"

# ---------------------------------------------------------------
# Case 6: server.remote runtime + hook defined under [cli.contract_*]
# Confirms the mismatch warning fires for contract-action sections too,
# not just for task-action sections.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_add.hooks.stray]
command = "true"
TOML

LOG6="$TEST_DIR/server6.log"
start_server_capture "$LOG6"
stop_server
LOG6_CONTENT="$(cat "$LOG6")"

echo "[6] server.remote runtime warns about foreign [cli.contract_*] hooks"
assert_contains "$LOG6_CONTENT" "do not match the active runtime" "mismatch warning present for [cli.contract_*]"
assert_contains "$LOG6_CONTENT" "cli" "mismatch warning mentions cli"

# ---------------------------------------------------------------
# Case 7: server.remote runtime + hook defined under [server.relay.contract_*]
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.relay.contract_dod_check.hooks.stray]
command = "true"
TOML

LOG7="$TEST_DIR/server7.log"
start_server_capture "$LOG7"
stop_server
LOG7_CONTENT="$(cat "$LOG7")"

echo "[7] server.remote runtime warns about foreign [server.relay.contract_*] hooks"
assert_contains "$LOG7_CONTENT" "do not match the active runtime" "mismatch warning present for [server.relay.contract_*]"
assert_contains "$LOG7_CONTENT" "server.relay" "mismatch warning mentions server.relay"

# ---------------------------------------------------------------
# Case 8: matching [server.remote.contract_*] hook — no mismatch warning
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[server.remote.contract_add.hooks.ok]
command = "true"
TOML

LOG8="$TEST_DIR/server8.log"
start_server_capture "$LOG8"
stop_server
LOG8_CONTENT="$(cat "$LOG8")"

echo "[8] matching [server.remote.contract_*] hook emits no mismatch warning"
assert_not_contains "$LOG8_CONTENT" "do not match the active runtime" "no mismatch warning for matching contract runtime"

test_summary
