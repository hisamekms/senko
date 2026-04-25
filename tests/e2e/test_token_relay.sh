#!/usr/bin/env bash
# E2E tests for SENKO_CLI_REMOTE_TOKEN relay: forwarding, auth failures, and token leak prevention
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
API_URL="http://127.0.0.1:$PORT"
MASTER_KEY=test-key
SERVER_LOG="$TEST_DIR/server.log"

# Start server with logging captured to file
RUST_LOG=info SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" \
  --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
  serve --port "$PORT" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"

TEST_TOKEN=$(create_test_user_key "$API_URL" "$MASTER_KEY")

# Helper: run CLI with SENKO_CLI_REMOTE_TOKEN
run_with_token() {
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

# Helper: run CLI without SENKO_CLI_REMOTE_TOKEN
run_without_token() {
  SENKO_CLI_REMOTE_URL="$API_URL" \
    "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

echo "=== Section 1: SENKO_CLI_REMOTE_TOKEN forwarded to upstream ==="

echo "[1] list with SENKO_CLI_REMOTE_TOKEN succeeds"
LIST=$(run_with_token task list)
assert_eq "0" "$(echo "$LIST" | jq '.items | length')" "list: empty initially"

echo "[2] add with SENKO_CLI_REMOTE_TOKEN succeeds"
TASK=$(run_with_token task add --title "Token Relay Task")
TASK_ID=$(echo "$TASK" | jq -r '.id')
assert_json_field "$TASK" '.title' "Token Relay Task" "add: title"

echo "[3] get with SENKO_CLI_REMOTE_TOKEN succeeds"
GOT=$(run_with_token task get "$TASK_ID")
assert_json_field "$GOT" '.title' "Token Relay Task" "get: title matches"

echo ""
echo "=== Section 2: Without SENKO_CLI_REMOTE_TOKEN, operations fail ==="

echo "[4] list without SENKO_CLI_REMOTE_TOKEN fails"
OUTPUT=$(run_without_token task list 2>&1 || true)
assert_contains "$OUTPUT" "authentication required" "list without token: auth error"

echo "[5] empty SENKO_CLI_REMOTE_TOKEN fails"
OUTPUT=$(SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" task list 2>&1 || true)
assert_contains "$OUTPUT" "authentication required" "list with empty token: auth error"

echo "[6] invalid SENKO_CLI_REMOTE_TOKEN fails"
OUTPUT=$(SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="invalid-token-xxxxx" \
  "$SENKO" --project-root "$TEST_PROJECT_ROOT" task list 2>&1 || true)
assert_contains "$OUTPUT" "authentication required" "list with invalid token: auth error"

echo ""
echo "=== Section 3: Token not leaked in logs ==="

# Generate additional log entries with the valid token
run_with_token task list >/dev/null 2>&1
run_with_token task get "$TASK_ID" >/dev/null 2>&1

# Give server a moment to flush logs
sleep 0.5

SERVER_LOGS=$(cat "$SERVER_LOG")

echo "[7] Server logs do not contain raw token"
assert_not_contains "$SERVER_LOGS" "$TEST_TOKEN" "logs: no raw token"

echo "[8] Server logs do not contain Bearer header value"
assert_not_contains "$SERVER_LOGS" "Bearer $TEST_TOKEN" "logs: no Bearer token"

echo "[9] Server logs contain expected entries (sanity check)"
assert_contains "$SERVER_LOGS" "http_request" "logs: contain http_request entries"

test_summary
