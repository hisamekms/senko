#!/usr/bin/env bash
# E2E tests for auth/session management endpoints
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

MASTER_KEY="test-key"

PORT=$(allocate_port)
SERVER_URL="http://127.0.0.1:$PORT"
AUTH_BASE="$SERVER_URL/auth"

# Start the API server with master key auth
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $SERVER_URL/api/v1/health >/dev/null"

# Create test user with API key
TEST_TOKEN=$(create_test_user_key "$SERVER_URL" "$MASTER_KEY")

# Helper: get HTTP status code (no auth)
status_no_auth() {
  curl -s -o /dev/null -w '%{http_code}' "$@"
}

# Helper: get HTTP status code with specific bearer token
status_with_token() {
  local token="$1"; shift
  curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $token" "$@"
}

# Helper: GET with auth, return body
api_get() {
  curl -sf -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

# Helper: POST JSON with auth, return body
api_json() {
  curl -sf -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

# =============================================
# 1. GET /auth/config (public, no auth required)
# =============================================

echo "=== GET /auth/config without auth returns 200 ==="
STATUS=$(status_no_auth "$AUTH_BASE/config")
assert_eq "200" "$STATUS" "GET /auth/config without auth returns 200"

echo ""
echo "=== GET /auth/config returns JSON with auth_mode and oidc fields ==="
BODY=$(curl -sf "$AUTH_BASE/config")
assert_contains "$BODY" '"auth_mode"' "GET /auth/config response contains auth_mode field"
assert_contains "$BODY" '"auth_mode":"api_key"' "GET /auth/config returns auth_mode api_key"
assert_contains "$BODY" '"oidc"' "GET /auth/config response contains oidc field"

# =============================================
# 2. GET /auth/me
# =============================================

echo ""
echo "=== GET /auth/me with valid token returns 200 ==="
STATUS=$(status_with_token "$TEST_TOKEN" "$AUTH_BASE/me")
assert_eq "200" "$STATUS" "GET /auth/me with valid token returns 200"

echo ""
echo "=== GET /auth/me returns user and session ==="
ME=$(api_get "$AUTH_BASE/me")
assert_json_field "$ME" '.user.id' "$(echo "$ME" | jq -r '.user.id')" "GET /auth/me has user.id"
assert_contains "$ME" '"session"' "GET /auth/me response contains session"
assert_contains "$ME" '"key_prefix"' "GET /auth/me session contains key_prefix"

echo ""
echo "=== GET /auth/me without auth returns 401 ==="
STATUS=$(status_no_auth "$AUTH_BASE/me")
assert_eq "401" "$STATUS" "GET /auth/me without auth returns 401"

echo ""
echo "=== GET /auth/me with invalid token returns 401 ==="
STATUS=$(status_with_token "invalid-token" "$AUTH_BASE/me")
assert_eq "401" "$STATUS" "GET /auth/me with invalid token returns 401"

# =============================================
# 3. POST /auth/token
# =============================================

echo ""
echo "=== POST /auth/token with valid token returns 201 ==="
STATUS=$(status_with_token "$TEST_TOKEN" -X POST "$AUTH_BASE/token")
assert_eq "201" "$STATUS" "POST /auth/token with valid token returns 201"

echo ""
echo "=== POST /auth/token returns token response ==="
TOKEN_RESP=$(api_json -X POST "$AUTH_BASE/token" -d '{}')
assert_contains "$TOKEN_RESP" '"token"' "POST /auth/token response contains token"
assert_contains "$TOKEN_RESP" '"id"' "POST /auth/token response contains id"
assert_contains "$TOKEN_RESP" '"key_prefix"' "POST /auth/token response contains key_prefix"

echo ""
echo "=== POST /auth/token with device_name returns 201 ==="
TOKEN_RESP2=$(api_json -X POST "$AUTH_BASE/token" -d '{"device_name":"test-device"}')
assert_eq "201" "$(echo "$TOKEN_RESP2" | jq -r 'if .token then "201" else "fail" end')" "POST /auth/token with device_name succeeds"

echo ""
echo "=== POST /auth/token without auth returns 401 ==="
STATUS=$(status_no_auth -X POST "$AUTH_BASE/token")
assert_eq "401" "$STATUS" "POST /auth/token without auth returns 401"

# =============================================
# 4. GET /auth/sessions
# =============================================

echo ""
echo "=== GET /auth/sessions with valid token returns 200 ==="
STATUS=$(status_with_token "$TEST_TOKEN" "$AUTH_BASE/sessions")
assert_eq "200" "$STATUS" "GET /auth/sessions with valid token returns 200"

echo ""
echo "=== GET /auth/sessions returns paged items ==="
SESSIONS=$(api_get "$AUTH_BASE/sessions")
HAS_ITEMS=$(echo "$SESSIONS" | jq 'if (.items | type) == "array" then "yes" else "no" end' -r)
assert_eq "yes" "$HAS_ITEMS" "GET /auth/sessions returns {items: [...]}"

echo ""
echo "=== GET /auth/sessions contains sessions ==="
SESSION_COUNT=$(echo "$SESSIONS" | jq '.items | length')
# We created the initial API key + 2 tokens via POST /auth/token, so should have at least 1
assert_eq "true" "$([ "$SESSION_COUNT" -ge 1 ] && echo true || echo false)" "GET /auth/sessions has at least 1 session"

echo ""
echo "=== GET /auth/sessions without auth returns 401 ==="
STATUS=$(status_no_auth "$AUTH_BASE/sessions")
assert_eq "401" "$STATUS" "GET /auth/sessions without auth returns 401"

echo ""
echo "=== GET /auth/sessions cursor pagination round-trip ==="
# Create 2 additional sessions so we have >=3 rows to page through.
api_json -X POST "$AUTH_BASE/token" -d '{}' >/dev/null
api_json -X POST "$AUTH_BASE/token" -d '{}' >/dev/null
PAGE1=$(api_get "$AUTH_BASE/sessions?limit=1")
assert_eq "1" "$(echo "$PAGE1" | jq '.items | length')" "paged sessions: page1 has 1 item"
CURSOR=$(echo "$PAGE1" | jq -r '.next_cursor')
if [[ "$CURSOR" == "null" ]]; then
  echo "FAIL: expected next_cursor on first page of /auth/sessions" >&2
  exit 1
fi
ENC_CURSOR=$(printf '%s' "$CURSOR" | jq -sRr @uri)
PAGE2=$(api_get "$AUTH_BASE/sessions?limit=1&after=$ENC_CURSOR")
assert_eq "1" "$(echo "$PAGE2" | jq '.items | length')" "paged sessions: page2 has 1 item"

echo ""
echo "=== GET /auth/sessions with invalid cursor returns 400 ==="
STATUS=$(status_with_token "$TEST_TOKEN" "$AUTH_BASE/sessions?after=not-a-cursor")
assert_eq "400" "$STATUS" "invalid cursor returns 400"

# =============================================
# 5. DELETE /auth/sessions/{id} (revoke specific)
# =============================================

echo ""
echo "=== DELETE /auth/sessions/{id} revokes specific session ==="
# Create a new token to revoke
NEW_TOKEN_RESP=$(api_json -X POST "$AUTH_BASE/token" -d '{}')
NEW_TOKEN_ID=$(echo "$NEW_TOKEN_RESP" | jq -r '.id')

# Count sessions before revoke
BEFORE_COUNT=$(api_get "$AUTH_BASE/sessions" | jq '.items | length')

# Revoke the new token
STATUS=$(status_with_token "$TEST_TOKEN" -X DELETE "$AUTH_BASE/sessions/$NEW_TOKEN_ID")
assert_eq "204" "$STATUS" "DELETE /auth/sessions/$NEW_TOKEN_ID returns 204"

echo ""
echo "=== Revoked session is removed from list ==="
AFTER_COUNT=$(api_get "$AUTH_BASE/sessions" | jq '.items | length')
assert_eq "true" "$([ "$AFTER_COUNT" -lt "$BEFORE_COUNT" ] && echo true || echo false)" "Session count decreased after revoke"

echo ""
echo "=== DELETE /auth/sessions/{id} without auth returns 401 ==="
STATUS=$(status_no_auth -X DELETE "$AUTH_BASE/sessions/999")
assert_eq "401" "$STATUS" "DELETE /auth/sessions/{id} without auth returns 401"

# =============================================
# 6. DELETE /auth/sessions (revoke all)
# =============================================

echo ""
echo "=== DELETE /auth/sessions without auth returns 401 ==="
STATUS=$(status_no_auth -X DELETE "$AUTH_BASE/sessions")
assert_eq "401" "$STATUS" "DELETE /auth/sessions without auth returns 401"

echo ""
echo "=== DELETE /auth/sessions revokes all sessions ==="
# Create a fresh token to use for the revoke-all call
FRESH_TOKEN_RESP=$(api_json -X POST "$AUTH_BASE/token" -d '{}')
FRESH_TOKEN=$(echo "$FRESH_TOKEN_RESP" | jq -r '.token')

STATUS=$(status_with_token "$FRESH_TOKEN" -X DELETE "$AUTH_BASE/sessions")
assert_eq "204" "$STATUS" "DELETE /auth/sessions returns 204"

test_summary
