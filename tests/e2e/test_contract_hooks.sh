#!/usr/bin/env bash
# E2E tests for contract-aggregate hooks under each runtime.
#
# Mirrors `test_http_hooks.sh`, but covers the six contract action keys
# (contract_add / contract_edit / contract_delete / contract_dod_check /
# contract_dod_uncheck / contract_note_add) across CLI, server.remote, and
# server.relay runtimes.

set -euo pipefail

source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

UPSTREAM_PID=""
RELAY_PID=""
MARKER_DIR="$TEST_DIR/hook-markers"
mkdir -p "$MARKER_DIR"

# --- Helper functions ---

write_config() {
  mkdir -p "$TEST_PROJECT_ROOT/.senko"
  cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.contract_add.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_add"
mode = "sync"

[cli.contract_edit.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_edit"
mode = "sync"

[cli.contract_delete.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_delete"
mode = "sync"

[cli.contract_dod_check.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_dod_check"
mode = "sync"

[cli.contract_dod_uncheck.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_dod_uncheck"
mode = "sync"

[cli.contract_note_add.hooks.cli_tag]
command = "touch $MARKER_DIR/cli_contract_note_add"
mode = "sync"

[server.remote.contract_add.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_add"
mode = "sync"

[server.remote.contract_edit.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_edit"
mode = "sync"

[server.remote.contract_delete.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_delete"
mode = "sync"

[server.remote.contract_dod_check.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_dod_check"
mode = "sync"

[server.remote.contract_dod_uncheck.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_dod_uncheck"
mode = "sync"

[server.remote.contract_note_add.hooks.srv_tag]
command = "touch $MARKER_DIR/srv_contract_note_add"
mode = "sync"

[server.relay.contract_add.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_add"
mode = "sync"

[server.relay.contract_edit.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_edit"
mode = "sync"

[server.relay.contract_delete.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_delete"
mode = "sync"

[server.relay.contract_dod_check.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_dod_check"
mode = "sync"

[server.relay.contract_dod_uncheck.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_dod_uncheck"
mode = "sync"

[server.relay.contract_note_add.hooks.relay_tag]
command = "touch $MARKER_DIR/relay_contract_note_add"
mode = "sync"
EOF
}

clear_markers() {
  rm -f "$MARKER_DIR"/*
}

MASTER_KEY=test-key

start_upstream() {
  UPSTREAM_PORT=$(allocate_port 0)
  UPSTREAM_URL="http://127.0.0.1:$UPSTREAM_PORT"
  SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$UPSTREAM_PORT" >/dev/null 2>&1 &
  UPSTREAM_PID=$!
  wait_for "upstream server ready" 10 "curl -sf $UPSTREAM_URL/api/v1/health >/dev/null"
  TEST_TOKEN=$(create_test_user_key "$UPSTREAM_URL" "$MASTER_KEY")
}

start_relay() {
  RELAY_PORT=$(allocate_port 1)
  RELAY_URL="http://127.0.0.1:$RELAY_PORT"
  SENKO_SERVER_RELAY_URL="$UPSTREAM_URL" \
  SENKO_SERVER_RELAY_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" serve --port "$RELAY_PORT" >/dev/null 2>&1 &
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

cleanup_full() {
  stop_servers
  cleanup_test_env
}
trap cleanup_full EXIT

run_http() {
  SENKO_CLI_REMOTE_URL="$UPSTREAM_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

run_relay() {
  SENKO_CLI_REMOTE_URL="$RELAY_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

assert_marker() {
  local marker="$1"
  local message="$2"
  if [[ -f "$MARKER_DIR/$marker" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message (missing marker: $marker)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_no_marker() {
  local marker="$1"
  local message="$2"
  if [[ ! -f "$MARKER_DIR/$marker" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message (unexpected marker: $marker)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

write_config

# ========================================
# Section 1: Direct CLI runtime — only [cli.*] hooks fire.
# ========================================
echo "--- Section 1: direct CLI runtime ---"

clear_markers

CID=$(run_lf --output json contract add \
  --title "CLI Contract" \
  --definition-of-done "[manual] item1" \
  --definition-of-done "[manual] item2" | jq -r '.id')
run_lf contract edit "$CID" --title "CLI Contract v2" >/dev/null 2>&1
run_lf contract dod check "$CID" 1 >/dev/null 2>&1
run_lf contract dod uncheck "$CID" 1 >/dev/null 2>&1
run_lf contract note add "$CID" --content "a note" >/dev/null 2>&1
run_lf contract delete "$CID" >/dev/null 2>&1

sleep 1

echo "[1.1] cli hooks fired for every contract write"
assert_marker "cli_contract_add" "cli_contract_add marker"
assert_marker "cli_contract_edit" "cli_contract_edit marker"
assert_marker "cli_contract_dod_check" "cli_contract_dod_check marker"
assert_marker "cli_contract_dod_uncheck" "cli_contract_dod_uncheck marker"
assert_marker "cli_contract_note_add" "cli_contract_note_add marker"
assert_marker "cli_contract_delete" "cli_contract_delete marker"

echo "[1.2] server.remote / server.relay hooks did NOT fire"
assert_no_marker "srv_contract_add" "no srv_contract_add"
assert_no_marker "srv_contract_dod_check" "no srv_contract_dod_check"
assert_no_marker "srv_contract_note_add" "no srv_contract_note_add"
assert_no_marker "relay_contract_add" "no relay_contract_add"
assert_no_marker "relay_contract_dod_check" "no relay_contract_dod_check"

# ========================================
# Section 2: server.remote runtime — [server.remote.*] hooks fire server-side.
# ========================================
echo "--- Section 2: server.remote runtime ---"

start_upstream
clear_markers

CID2=$(run_http --output json contract add \
  --title "HTTP Contract" \
  --definition-of-done "[manual] a" \
  --definition-of-done "[manual] b" | jq -r '.id')
run_http contract edit "$CID2" --title "HTTP Contract v2" >/dev/null 2>&1
run_http contract dod check "$CID2" 1 >/dev/null 2>&1
run_http contract dod uncheck "$CID2" 1 >/dev/null 2>&1
run_http contract note add "$CID2" --content "server note" >/dev/null 2>&1
run_http contract delete "$CID2" >/dev/null 2>&1

sleep 1

echo "[2.1] server.remote hooks fired"
assert_marker "srv_contract_add" "srv_contract_add marker"
assert_marker "srv_contract_edit" "srv_contract_edit marker"
assert_marker "srv_contract_dod_check" "srv_contract_dod_check marker"
assert_marker "srv_contract_dod_uncheck" "srv_contract_dod_uncheck marker"
assert_marker "srv_contract_note_add" "srv_contract_note_add marker"
assert_marker "srv_contract_delete" "srv_contract_delete marker"

echo "[2.2] cli hooks also fired (CLI client process ran against remote)"
# CLI client itself is in cli runtime, so cli.contract_* hooks fire there as well.
assert_marker "cli_contract_add" "cli_contract_add (from http client)"

# ========================================
# Section 3: server.relay runtime — [server.relay.*] hooks fire on the relay.
# ========================================
echo "--- Section 3: server.relay runtime ---"

start_relay
clear_markers

# Go through the relay server instead of directly to upstream. The relay
# uses RemoteContractOperations and fires hooks under the server.relay runtime.
CID3=$(run_relay --output json contract add \
  --title "Relay Contract" \
  --definition-of-done "[manual] x" \
  --definition-of-done "[manual] y" | jq -r '.id')
run_relay contract edit "$CID3" --title "Relay Contract v2" >/dev/null 2>&1
run_relay contract dod check "$CID3" 1 >/dev/null 2>&1
run_relay contract dod uncheck "$CID3" 1 >/dev/null 2>&1
run_relay contract note add "$CID3" --content "relay note" >/dev/null 2>&1
run_relay contract delete "$CID3" >/dev/null 2>&1

sleep 1

echo "[3.1] server.relay hooks fired on relay server"
assert_marker "relay_contract_add" "relay_contract_add marker"
assert_marker "relay_contract_edit" "relay_contract_edit marker"
assert_marker "relay_contract_dod_check" "relay_contract_dod_check marker"
assert_marker "relay_contract_dod_uncheck" "relay_contract_dod_uncheck marker"
assert_marker "relay_contract_note_add" "relay_contract_note_add marker"
assert_marker "relay_contract_delete" "relay_contract_delete marker"

echo "[3.2] server.remote hooks also fired (upstream processed the forwarded write)"
# The relay forwards to upstream, which runs in server.remote runtime.
assert_marker "srv_contract_add" "srv_contract_add (relay -> upstream)"

stop_servers

# ========================================
# Section 4: senko hooks test contract_* (dry-run)
# ========================================
echo "--- Section 4: senko hooks test contract_* dry-run ---"

# Each contract event must be accepted by `senko hooks test --dry-run` and
# produce a JSON envelope with the corresponding event name.
for evt in contract_add contract_edit contract_delete contract_dod_check contract_dod_uncheck contract_note_add; do
  OUT="$(run_lf hooks test "$evt" --dry-run 2>/dev/null || true)"
  EVT="$(echo "$OUT" | jq -r '.event.event' 2>/dev/null || echo "")"
  assert_eq "$evt" "$EVT" "hooks test $evt dry-run event name"
done

echo "[4.1] invalid contract-style event still errors"
INVALID_OUTPUT="$(run_lf hooks test contract_bogus 2>&1 || true)"
assert_contains "$INVALID_OUTPUT" "unknown event" "contract_bogus: unknown event error"

test_summary
