#!/usr/bin/env bash
# E2E tests for the --dev-no-auth flag (server.auth.dev_bypass)
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
BASE="http://127.0.0.1:$PORT"
APIBASE="$BASE/api/v1"
PBASE="$APIBASE/projects/1"

echo "=== SENKO_ENV=production must refuse to start ==="
REFUSE_LOG="$TEST_DIR/refuse.log"
set +e
SENKO_ENV=production "$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" \
  serve --port "$PORT" --dev-no-auth >"$REFUSE_LOG" 2>&1
REFUSE_EXIT=$?
set -e
[[ "$REFUSE_EXIT" -ne 0 ]] \
  && { echo "  PASS: production guard exited non-zero"; PASS_COUNT=$((PASS_COUNT+1)); } \
  || { echo "  FAIL: production guard should exit non-zero (got $REFUSE_EXIT)"; FAIL_COUNT=$((FAIL_COUNT+1)); }
assert_contains "$(cat "$REFUSE_LOG")" "production" "guard error mentions production"

echo ""
echo "=== Boot in dev_bypass mode ==="
SERVER_LOG="$TEST_DIR/server.log"
"$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" \
  serve --port "$PORT" --dev-no-auth >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $APIBASE/health >/dev/null"

# Banner — wait briefly for the warn log to flush, then assert.
sleep 0.2
assert_contains "$(cat "$SERVER_LOG")" "DO NOT USE IN PRODUCTION" "boot banner present"

echo ""
echo "=== /auth/config advertises dev_bypass ==="
CONFIG=$(curl -sf "$BASE/auth/config")
assert_json_field "$CONFIG" '.auth_mode' "dev_bypass" "auth_mode is dev_bypass"

echo ""
echo "=== Authenticated endpoints work without Authorization header ==="
# The synthetic user is master, so project-membership checks are bypassed.
TASKS_STATUS=$(curl -s -o /dev/null -w '%{http_code}' "$PBASE/tasks")
assert_eq "200" "$TASKS_STATUS" "GET /tasks without auth returns 200"

CREATED=$(curl -sf -X POST -H "Content-Type: application/json" \
  "$PBASE/tasks" -d '{"title":"Bypass Task"}')
assert_json_field "$CREATED" '.title' "Bypass Task" "POST /tasks succeeds without auth"

# /auth/me returns the synthetic user
ME=$(curl -sf "$BASE/auth/me")
assert_json_field "$ME" '.user.username' "dev-bypass" "/auth/me returns synthetic username"
assert_json_field "$ME" '.session' "null" "/auth/me has no session under bypass"

echo ""
echo "=== /auth/token is disabled under bypass ==="
TOKEN_STATUS=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  -H "Content-Type: application/json" "$BASE/auth/token" -d '{}')
assert_eq "501" "$TOKEN_STATUS" "/auth/token returns 501 Not Implemented"

test_summary
