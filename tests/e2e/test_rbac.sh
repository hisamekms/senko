#!/usr/bin/env bash
# E2E tests for project-level RBAC (Role-Based Access Control)
# Tests View/Edit/Admin permission levels across Owner/Member/Viewer roles.
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

MASTER_KEY="test-master-key"
PORT=$(allocate_port 0)
BASE="http://127.0.0.1:$PORT/api/v1"
PROJECT_ID=1

# Start the API server with master key auth
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" \
  --project-root "$TEST_PROJECT_ROOT" \
  --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
  serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

# --- Helpers ---

status_no_auth() {
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" "$@"
}

status_with_token() {
  local token="$1"; shift
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" \
    -H "Authorization: Bearer $token" "$@"
}

api_get() {
  local token="$1"; shift
  curl -sf -H "Authorization: Bearer $token" "$@"
}

# Create a user via master key, echo user_id
mk_user() {
  local username="$1"
  curl -sf -X POST "$BASE/users" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $MASTER_KEY" \
    -d "{\"username\":\"$username\"}" | jq -r '.id'
}

# Create API key for a user via master key, echo key
mk_api_key() {
  local user_id="$1"
  curl -sf -X POST "$BASE/users/$user_id/api-keys" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $MASTER_KEY" \
    -d '{}' | jq -r '.key'
}

# =============================================
# Setup: Create users and assign roles
# =============================================

OWNER_UID=$(mk_user "owner-user")
MEMBER_UID=$(mk_user "member-user")
VIEWER_UID=$(mk_user "viewer-user")
NONMEMBER_UID=$(mk_user "nonmember-user")

OWNER_KEY=$(mk_api_key "$OWNER_UID")
MEMBER_KEY=$(mk_api_key "$MEMBER_UID")
VIEWER_KEY=$(mk_api_key "$VIEWER_UID")
NONMEMBER_KEY=$(mk_api_key "$NONMEMBER_UID")

# Add owner to default project via CLI (bypasses API auth)
run_lf project members add --user-id "$OWNER_UID" --role owner >/dev/null

# Owner adds member and viewer via API
echo "=== Setup: Owner adds member to project ==="
STATUS=$(status_with_token "$OWNER_KEY" -X POST "$BASE/projects/$PROJECT_ID/members" \
  -d "{\"user_id\":$MEMBER_UID,\"role\":\"member\"}")
assert_eq "201" "$STATUS" "Owner can add member via API"

echo ""
echo "=== Setup: Owner adds viewer to project ==="
STATUS=$(status_with_token "$OWNER_KEY" -X POST "$BASE/projects/$PROJECT_ID/members" \
  -d "{\"user_id\":$VIEWER_UID,\"role\":\"viewer\"}")
assert_eq "201" "$STATUS" "Owner can add viewer via API"

# =============================================
# 1. Admin-only: POST /members (add member)
# =============================================

echo ""
echo "=== POST /members: Member cannot add users (403) ==="
STATUS=$(status_with_token "$MEMBER_KEY" -X POST "$BASE/projects/$PROJECT_ID/members" \
  -d "{\"user_id\":$NONMEMBER_UID,\"role\":\"viewer\"}")
assert_eq "403" "$STATUS" "Member cannot add members"

echo ""
echo "=== POST /members: Viewer cannot add users (403) ==="
STATUS=$(status_with_token "$VIEWER_KEY" -X POST "$BASE/projects/$PROJECT_ID/members" \
  -d "{\"user_id\":$NONMEMBER_UID,\"role\":\"viewer\"}")
assert_eq "403" "$STATUS" "Viewer cannot add members"

# =============================================
# 2. Admin-only: PUT /members/{user_id} (update role)
# =============================================

echo ""
echo "=== PUT /members: Owner can update role (200) ==="
STATUS=$(status_with_token "$OWNER_KEY" -X PUT "$BASE/projects/$PROJECT_ID/members/$VIEWER_UID" \
  -d '{"role":"viewer"}')
assert_eq "200" "$STATUS" "Owner can update member role"

echo ""
echo "=== PUT /members: Member cannot update role (403) ==="
STATUS=$(status_with_token "$MEMBER_KEY" -X PUT "$BASE/projects/$PROJECT_ID/members/$VIEWER_UID" \
  -d '{"role":"member"}')
assert_eq "403" "$STATUS" "Member cannot update roles"

echo ""
echo "=== PUT /members: Viewer cannot update role (403) ==="
STATUS=$(status_with_token "$VIEWER_KEY" -X PUT "$BASE/projects/$PROJECT_ID/members/$MEMBER_UID" \
  -d '{"role":"viewer"}')
assert_eq "403" "$STATUS" "Viewer cannot update roles"

# =============================================
# 3. View-level: GET /members (list members)
# =============================================

echo ""
echo "=== GET /members: Owner can list (200) ==="
STATUS=$(status_with_token "$OWNER_KEY" "$BASE/projects/$PROJECT_ID/members")
assert_eq "200" "$STATUS" "Owner can list members"

echo ""
echo "=== GET /members: Member can list (200) ==="
STATUS=$(status_with_token "$MEMBER_KEY" "$BASE/projects/$PROJECT_ID/members")
assert_eq "200" "$STATUS" "Member can list members"

echo ""
echo "=== GET /members: Viewer can list (200) ==="
STATUS=$(status_with_token "$VIEWER_KEY" "$BASE/projects/$PROJECT_ID/members")
assert_eq "200" "$STATUS" "Viewer can list members"

echo ""
echo "=== GET /members: Non-member gets 403 ==="
STATUS=$(status_with_token "$NONMEMBER_KEY" "$BASE/projects/$PROJECT_ID/members")
assert_eq "403" "$STATUS" "Non-member cannot list members"

echo ""
echo "=== GET /members: No auth gets 401 ==="
STATUS=$(status_no_auth "$BASE/projects/$PROJECT_ID/members")
assert_eq "401" "$STATUS" "Unauthenticated cannot list members"

# =============================================
# 4. View-level: GET /members/{user_id}
# =============================================

echo ""
echo "=== GET /members/{user_id}: Owner can get specific member (200) ==="
BODY=$(api_get "$OWNER_KEY" "$BASE/projects/$PROJECT_ID/members/$MEMBER_UID")
assert_json_field "$BODY" '.user_id' "$MEMBER_UID" "Response has correct user_id"
assert_json_field "$BODY" '.role' "member" "Response has correct role"
assert_contains "$BODY" '"project_id"' "Response contains project_id"
assert_contains "$BODY" '"id"' "Response contains id"
assert_contains "$BODY" '"created_at"' "Response contains created_at"

# Task 318: member response now carries a nested `user` object with
# minimal identity fields (id / name / display_name). sub and email are
# deliberately omitted.
assert_json_field "$BODY" '.user.id' "$MEMBER_UID" "Response has nested user.id"
assert_json_field "$BODY" '.user.name' "member-user" "Response has nested user.name"
MEMBER_HAS_SUB=$(echo "$BODY" | jq 'has("user") and (.user | has("sub"))')
assert_eq "false" "$MEMBER_HAS_SUB" "nested user does not expose sub"
MEMBER_HAS_EMAIL=$(echo "$BODY" | jq 'has("user") and (.user | has("email"))')
assert_eq "false" "$MEMBER_HAS_EMAIL" "nested user does not expose email"

echo ""
echo "=== GET /members/{user_id}: Member can get specific member (200) ==="
STATUS=$(status_with_token "$MEMBER_KEY" "$BASE/projects/$PROJECT_ID/members/$OWNER_UID")
assert_eq "200" "$STATUS" "Member can get specific member"

echo ""
echo "=== GET /members/{user_id}: Viewer can get specific member (200) ==="
STATUS=$(status_with_token "$VIEWER_KEY" "$BASE/projects/$PROJECT_ID/members/$OWNER_UID")
assert_eq "200" "$STATUS" "Viewer can get specific member"

echo ""
echo "=== GET /members/{user_id}: Non-member gets 403 ==="
STATUS=$(status_with_token "$NONMEMBER_KEY" "$BASE/projects/$PROJECT_ID/members/$OWNER_UID")
assert_eq "403" "$STATUS" "Non-member cannot get specific member"

echo ""
echo "=== GET /members/{user_id}: No auth gets 401 ==="
STATUS=$(status_no_auth "$BASE/projects/$PROJECT_ID/members/$OWNER_UID")
assert_eq "401" "$STATUS" "Unauthenticated cannot get specific member"

# =============================================
# 5. View-level: GET /projects/{id}
# =============================================

echo ""
echo "=== GET /projects/{id}: Owner can access (200) ==="
STATUS=$(status_with_token "$OWNER_KEY" "$BASE/projects/$PROJECT_ID")
assert_eq "200" "$STATUS" "Owner can access project"

echo ""
echo "=== GET /projects/{id}: Member can access (200) ==="
STATUS=$(status_with_token "$MEMBER_KEY" "$BASE/projects/$PROJECT_ID")
assert_eq "200" "$STATUS" "Member can access project"

echo ""
echo "=== GET /projects/{id}: Viewer can access (200) ==="
STATUS=$(status_with_token "$VIEWER_KEY" "$BASE/projects/$PROJECT_ID")
assert_eq "200" "$STATUS" "Viewer can access project"

echo ""
echo "=== GET /projects/{id}: Non-member gets 403 ==="
STATUS=$(status_with_token "$NONMEMBER_KEY" "$BASE/projects/$PROJECT_ID")
assert_eq "403" "$STATUS" "Non-member cannot access project"

# =============================================
# 6. Admin-only: DELETE /members/{user_id}
# =============================================

echo ""
echo "=== DELETE /members: Member cannot remove (403) ==="
STATUS=$(status_with_token "$MEMBER_KEY" -X DELETE "$BASE/projects/$PROJECT_ID/members/$VIEWER_UID")
assert_eq "403" "$STATUS" "Member cannot remove members"

echo ""
echo "=== DELETE /members: Viewer cannot remove (403) ==="
STATUS=$(status_with_token "$VIEWER_KEY" -X DELETE "$BASE/projects/$PROJECT_ID/members/$MEMBER_UID")
assert_eq "403" "$STATUS" "Viewer cannot remove members"

echo ""
echo "=== DELETE /members: Owner can remove viewer (204) ==="
STATUS=$(status_with_token "$OWNER_KEY" -X DELETE "$BASE/projects/$PROJECT_ID/members/$VIEWER_UID")
assert_eq "204" "$STATUS" "Owner can remove members"

# Re-add viewer for subsequent tests if needed
status_with_token "$OWNER_KEY" -X POST "$BASE/projects/$PROJECT_ID/members" \
  -d "{\"user_id\":$VIEWER_UID,\"role\":\"viewer\"}" >/dev/null

# =============================================
# 7. Non-master caller: POST /users → 403
# =============================================
# With is_master on AuthUser, a non-master caller is always Forbidden on
# /users regardless of whether master_key is configured. The old 501
# semantics (NotImplemented when master_key unset) no longer apply.

echo ""
echo "=== POST /users with non-master OIDC-mode server returns 403 ==="
# Stop current server
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true

# Start new server with OIDC config (enables auth) but NO master key
PORT2=$(allocate_port 1)
BASE2="http://127.0.0.1:$PORT2/api/v1"

SENKO_OIDC_ISSUER_URL="https://fake.example.com" \
SENKO_OIDC_CLIENT_ID="fake-client" \
"$SENKO" \
  --project-root "$TEST_PROJECT_ROOT" \
  --db-path "$TEST_PROJECT_ROOT/.senko/data.db" \
  serve --port "$PORT2" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server (no master key) ready" 10 "curl -sf $BASE2/health >/dev/null"

# OWNER_KEY was created on the previous server, so it's invalid here — we
# expect 401 rather than 403 because the JWT-mode provider rejects it.
STATUS=$(status_with_token "$OWNER_KEY" -X POST "$BASE2/users" \
  -d '{"username":"should-fail"}')
assert_eq "401" "$STATUS" "POST /users with invalid token returns 401"

test_summary
