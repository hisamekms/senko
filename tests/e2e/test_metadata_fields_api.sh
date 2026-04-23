#!/usr/bin/env bash
# E2E tests for metadata field CRUD API endpoints
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PORT=$(allocate_port)
BASE="http://127.0.0.1:$PORT/api/v1"
PBASE="$BASE/projects/1"

# Start the API server in background
MASTER_KEY=test-key
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "${SENKO_DB_ARGS[@]}" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

# Wait for server to be ready
wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

# Create a real user and API key
TEST_TOKEN=$(create_test_user_key "http://127.0.0.1:$PORT" "$MASTER_KEY")

# --- Helpers ---
api_get() {
  curl -sf -H "Authorization: Bearer $TEST_TOKEN" "$@"
}
api_json() {
  curl -sf -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}
api_status() {
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

echo "=== Create metadata field ==="
F1=$(api_json -X POST "$PBASE/metadata-fields" -d '{"name":"sprint","field_type":"string","description":"Sprint name"}')
assert_json_field "$F1" '.name' "sprint" "created field name"
assert_json_field "$F1" '.field_type' "string" "created field type"
assert_json_field "$F1" '.required_on_complete' "false" "default required_on_complete"
assert_json_field "$F1" '.description' "Sprint name" "created field description"

echo ""
echo "=== Create second field ==="
F2=$(api_json -X POST "$PBASE/metadata-fields" -d '{"name":"story-points","field_type":"number","required_on_complete":true}')
assert_json_field "$F2" '.name' "story-points" "second field name"
assert_json_field "$F2" '.field_type' "number" "second field type"
assert_json_field "$F2" '.required_on_complete' "true" "required_on_complete is true"

echo ""
echo "=== List metadata fields ==="
LIST=$(api_get "$PBASE/metadata-fields")
assert_eq "2" "$(echo "$LIST" | jq '.items | length')" "list returns 2 fields"

echo ""
echo "=== Delete metadata field by name ==="
DEL_STATUS=$(api_status -X DELETE "$PBASE/metadata-fields/sprint")
assert_eq "204" "$DEL_STATUS" "delete returns 204"

echo ""
echo "=== List after delete ==="
LIST2=$(api_get "$PBASE/metadata-fields")
assert_eq "1" "$(echo "$LIST2" | jq '.items | length')" "list returns 1 field after delete"
assert_json_field "$(echo "$LIST2" | jq '.items[0]')" '.name' "story-points" "remaining field is story-points"

echo ""
echo "=== Create duplicate name (conflict) ==="
STATUS_DUP=$(api_status -X POST "$PBASE/metadata-fields" -d '{"name":"story-points","field_type":"string"}')
assert_eq "409" "$STATUS_DUP" "duplicate name returns 409"

echo ""
echo "=== Delete nonexistent field ==="
STATUS_NOT_FOUND=$(api_status -X DELETE "$PBASE/metadata-fields/nonexistent")
assert_eq "404" "$STATUS_NOT_FOUND" "delete nonexistent returns 404"

echo ""
echo "=== Create with invalid name ==="
STATUS_INVALID=$(api_status -X POST "$PBASE/metadata-fields" -d '{"name":"InvalidName","field_type":"string"}')
assert_eq "400" "$STATUS_INVALID" "invalid name returns 400"

test_summary
