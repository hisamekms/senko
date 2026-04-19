#!/usr/bin/env bash
# E2E tests for trusted_headers authentication mode
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
BASE="http://127.0.0.1:$PORT/api/v1"
AUTH_BASE="http://127.0.0.1:$PORT/auth"
PBASE="$BASE/projects/1"

# Start the API server with trusted_headers auth.
# master_group=admin is configured so that callers whose groups_header
# contains "admin" are promoted to master (required for /users endpoints).
SENKO_AUTH_TRUSTED_HEADERS_SUBJECT_HEADER="x-senko-user-sub" \
SENKO_AUTH_TRUSTED_HEADERS_NAME_HEADER="x-senko-user-name" \
SENKO_AUTH_TRUSTED_HEADERS_EMAIL_HEADER="x-senko-user-email" \
SENKO_AUTH_TRUSTED_HEADERS_GROUPS_HEADER="x-senko-user-groups" \
SENKO_AUTH_TRUSTED_HEADERS_SCOPE_HEADER="x-senko-user-scope" \
SENKO_AUTH_TRUSTED_HEADERS_MASTER_GROUP="admin" \
"$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

# --- Helpers ---

# GET with trusted headers (alice)
api_get() {
  curl -sf \
    -H "x-senko-user-sub: alice" \
    -H "x-senko-user-name: Alice Smith" \
    -H "x-senko-user-email: alice@example.com" \
    -H "x-senko-user-groups: admin,dev" \
    -H "x-senko-user-scope: read,write" \
    "$@"
}

# POST/PUT/DELETE JSON with trusted headers (alice)
api_json() {
  curl -sf \
    -H "Content-Type: application/json" \
    -H "x-senko-user-sub: alice" \
    -H "x-senko-user-name: Alice Smith" \
    -H "x-senko-user-email: alice@example.com" \
    -H "x-senko-user-groups: admin,dev" \
    -H "x-senko-user-scope: read,write" \
    "$@"
}

# HTTP status code with trusted headers (alice)
api_status() {
  curl -s -o /dev/null -w '%{http_code}' \
    -H "Content-Type: application/json" \
    -H "x-senko-user-sub: alice" \
    -H "x-senko-user-name: Alice Smith" \
    -H "x-senko-user-email: alice@example.com" \
    -H "x-senko-user-groups: admin,dev" \
    -H "x-senko-user-scope: read,write" \
    "$@"
}

# HTTP status code with no auth headers
status_no_auth() {
  curl -s -o /dev/null -w '%{http_code}' "$@"
}

# =============================================
# 1. GET /auth/config returns trusted_headers mode
# =============================================

echo "=== GET /auth/config returns trusted_headers mode ==="
CONFIG=$(curl -sf "$AUTH_BASE/config")
assert_contains "$CONFIG" '"auth_mode":"trusted_headers"' "auth_mode is trusted_headers"

# =============================================
# 2. Auth success with x-senko-user-sub header
# =============================================

echo ""
echo "=== Auth success with subject header ==="
STATUS=$(api_status "$BASE/users")
assert_eq "200" "$STATUS" "GET /api/v1/users with subject header returns 200"

# =============================================
# 3. Auth failure without subject header (401)
# =============================================

echo ""
echo "=== Auth failure without any headers ==="
STATUS=$(status_no_auth "$BASE/users")
assert_eq "401" "$STATUS" "GET /api/v1/users without headers returns 401"

echo ""
echo "=== Auth failure with non-subject headers only ==="
STATUS=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "x-senko-user-name: No Subject" \
  -H "x-senko-user-email: nosub@example.com" \
  "$BASE/users")
assert_eq "401" "$STATUS" "GET /api/v1/users without subject header returns 401"

# =============================================
# 4. Auto-provisioning verification
# =============================================

echo ""
echo "=== Auto-provisioned user has correct fields ==="
USERS=$(api_get "$BASE/users")
ALICE=$(echo "$USERS" | jq '.[] | select(.sub == "alice")')
assert_json_field "$ALICE" '.sub' "alice" "auto-provisioned sub"
assert_json_field "$ALICE" '.username' "Alice Smith" "auto-provisioned username from name_header"
assert_json_field "$ALICE" '.display_name' "Alice Smith" "auto-provisioned display_name"
assert_json_field "$ALICE" '.email' "alice@example.com" "auto-provisioned email"
ALICE_ID=$(echo "$ALICE" | jq -r '.id')

# =============================================
# 5. Second user auto-provisioning
# =============================================
# bob is a non-master caller (no "admin" group), so hit /auth/me — a non-
# master endpoint — to trigger auto-provisioning.

echo ""
echo "=== Second user auto-provisioning (via /auth/me) ==="
STATUS=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "x-senko-user-sub: bob" \
  -H "x-senko-user-name: Bob Jones" \
  -H "x-senko-user-email: bob@example.com" \
  "$AUTH_BASE/me")
assert_eq "200" "$STATUS" "bob auto-provisioned via /auth/me"

USERS=$(api_get "$BASE/users")
BOB=$(echo "$USERS" | jq '.[] | select(.sub == "bob")')
assert_json_field "$BOB" '.sub' "bob" "bob sub"
assert_json_field "$BOB" '.username' "Bob Jones" "bob username from name_header"
assert_json_field "$BOB" '.display_name' "Bob Jones" "bob display_name"
assert_json_field "$BOB" '.email' "bob@example.com" "bob email"

# =============================================
# 6. Existing user is not duplicated
# =============================================

echo ""
echo "=== Existing user is not duplicated ==="
# Make another request as alice
api_get "$BASE/users" >/dev/null
USERS=$(api_get "$BASE/users")
ALICE_COUNT=$(echo "$USERS" | jq '[.[] | select(.sub == "alice")] | length')
assert_eq "1" "$ALICE_COUNT" "alice is not duplicated"

# =============================================
# 7. Groups/scope headers do not break auth
# =============================================

echo ""
echo "=== Auth succeeds with groups and scope headers ==="
STATUS=$(api_status "$BASE/users")
assert_eq "200" "$STATUS" "request with groups/scope headers succeeds"

echo ""
echo "=== Auth succeeds with only subject header (no groups/scope) — non-master endpoint ==="
# No groups header → not master, so hit /auth/me instead of /users.
STATUS=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "x-senko-user-sub: alice" \
  "$AUTH_BASE/me")
assert_eq "200" "$STATUS" "request with only subject header succeeds"

# =============================================
# 7b. master_group promotion
# =============================================

echo ""
echo "=== master_group grants master (GET /users returns 200 for admin group) ==="
# alice has "admin,dev" groups; admin matches master_group → 200
STATUS=$(api_status "$BASE/users")
assert_eq "200" "$STATUS" "caller in master_group can list users"

echo ""
echo "=== Non-master caller gets 403 on /users ==="
STATUS=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "x-senko-user-sub: carol" \
  -H "x-senko-user-groups: dev,ops" \
  "$BASE/users")
assert_eq "403" "$STATUS" "caller outside master_group is forbidden on /users"

# =============================================
# 8. Task CRUD with trusted headers auth
# =============================================

echo ""
echo "=== Setup: Add alice to project ==="
run_lf project members add --user-id "$ALICE_ID" --role owner >/dev/null

echo ""
echo "=== Create task ==="
TASK=$(api_json -X POST "$PBASE/tasks" -d '{"title":"Trusted Header Task","description":"Created via trusted headers"}')
assert_json_field "$TASK" '.title' "Trusted Header Task" "create task title"
assert_json_field "$TASK" '.status' "draft" "create task status is draft"
TASK_ID=$(echo "$TASK" | jq -r '.id')

echo ""
echo "=== Get task ==="
GOT=$(api_get "$PBASE/tasks/$TASK_ID")
assert_json_field "$GOT" '.id' "$TASK_ID" "get task by id"
assert_json_field "$GOT" '.title' "Trusted Header Task" "get task title"

echo ""
echo "=== List tasks ==="
LIST=$(api_get "$PBASE/tasks")
assert_eq "1" "$(echo "$LIST" | jq 'length')" "list returns 1 task"

echo ""
echo "=== Edit task ==="
EDITED=$(api_json -X PUT "$PBASE/tasks/$TASK_ID" -d '{"title":"Updated via Headers"}')
assert_json_field "$EDITED" '.title' "Updated via Headers" "edit task title"

echo ""
echo "=== Status transitions: ready -> start -> complete ==="
READY=$(api_json -X POST "$PBASE/tasks/$TASK_ID/ready" -d '{}')
assert_json_field "$READY" '.status' "todo" "ready transitions to todo"

STARTED=$(api_json -X POST "$PBASE/tasks/$TASK_ID/start" -d '{}')
assert_json_field "$STARTED" '.status' "in_progress" "start transitions to in_progress"

COMPLETED=$(api_json -X POST "$PBASE/tasks/$TASK_ID/complete" -d '{}')
assert_json_field "$COMPLETED" '.task.status' "completed" "complete transitions to completed"

echo ""
echo "=== Delete task ==="
TASK2=$(api_json -X POST "$PBASE/tasks" -d '{"title":"To Delete"}')
TASK2_ID=$(echo "$TASK2" | jq -r '.id')
DEL_STATUS=$(api_status -X DELETE "$PBASE/tasks/$TASK2_ID")
assert_eq "204" "$DEL_STATUS" "delete returns 204"
GET_DEL_STATUS=$(api_status "$PBASE/tasks/$TASK2_ID")
assert_eq "404" "$GET_DEL_STATUS" "deleted task returns 404"

# =============================================
# 9. GET /auth/me with trusted headers
# =============================================

echo ""
echo "=== GET /auth/me returns 200 with trusted headers ==="
ME_STATUS=$(api_status "$AUTH_BASE/me")
assert_eq "200" "$ME_STATUS" "GET /auth/me with trusted headers returns 200"

echo ""
echo "=== GET /auth/me returns user info and null session ==="
ME=$(api_get "$AUTH_BASE/me")
assert_json_field "$ME" '.user.sub' "alice" "GET /auth/me user.sub"
assert_json_field "$ME" '.user.username' "Alice Smith" "GET /auth/me user.username"
assert_contains "$ME" '"session":null' "GET /auth/me session is null in trusted_headers mode"

echo ""
echo "=== GET /auth/me without auth returns 401 ==="
ME_NO_AUTH_STATUS=$(status_no_auth "$AUTH_BASE/me")
assert_eq "401" "$ME_NO_AUTH_STATUS" "GET /auth/me without auth returns 401"

test_summary
