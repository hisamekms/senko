#!/usr/bin/env bash
# e2e test: Input size validation for API fields
# Validates that oversized inputs are rejected with clear error messages
# via both CLI and HTTP API.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

# Helper: generate a string of N characters
gen_chars() {
  printf '%0.sa' $(seq 1 "$1")
}

echo "--- Test: Input Size Validation (CLI) ---"

# ===== [1] Title: at limit (500 chars) → OK =====

echo "[1] Title at limit (500 chars)"
TITLE_500="$(gen_chars 500)"
OUT="$(run_lf --output json task add --title "$TITLE_500")"
assert_json_field "$OUT" '.title' "$TITLE_500" "title at 500 chars accepted"

# ===== [2] Title: over limit (501 chars) → error =====

echo "[2] Title over limit (501 chars)"
TITLE_501="$(gen_chars 501)"
ERR="$(run_lf --output json task add --title "$TITLE_501" 2>&1 || true)"
assert_exit_code 1 run_lf task add --title "$TITLE_501"
assert_contains "$ERR" "500" "error mentions max length"

# ===== [3] Tag too long (101 chars) → error =====

echo "[3] Tag over limit (101 chars)"
TAG_101="$(gen_chars 101)"
assert_exit_code 1 run_lf task add --title "Tag Test" --tag "$TAG_101"

# ===== [4] Too many tags (21) → error =====

echo "[4] Too many tags (21)"
TAG_ARGS=""
for i in $(seq 1 21); do
  TAG_ARGS="$TAG_ARGS --tag tag$i"
done
assert_exit_code 1 run_lf task add --title "Many Tags" $TAG_ARGS

# ===== [5] Tags at limit (20) → OK =====

echo "[5] Tags at limit (20)"
TAG_ARGS=""
for i in $(seq 1 20); do
  TAG_ARGS="$TAG_ARGS --tag t$i"
done
OUT="$(run_lf --output json task add --title "Twenty Tags" $TAG_ARGS)"
TAG_COUNT="$(echo "$OUT" | jq '.tags | length')"
assert_eq "20" "$TAG_COUNT" "20 tags accepted"

# ===== [6] DoD item too long (501 chars) → error =====

echo "[6] DoD item over limit (501 chars)"
DOD_501="$(gen_chars 501)"
assert_exit_code 1 run_lf task add --title "DoD Test" --definition-of-done "$DOD_501"

# ===== [7] Edit: title over limit → error =====

echo "[7] Edit: title over limit"
OUT="$(run_lf --output json task add --title "Edit Target")"
EDIT_ID="$(echo "$OUT" | jq -r '.id')"
assert_exit_code 1 run_lf task edit "$EDIT_ID" --title "$TITLE_501"

# ===== [8] Edit: add-tag over limit → error =====

echo "[8] Edit: add-tag over limit"
assert_exit_code 1 run_lf task edit "$EDIT_ID" --add-tag "$TAG_101"

# ===== [9] Cancel: reason over limit (50001 chars) → error =====

echo "[9] Cancel: reason over limit"
REASON_LONG="$(gen_chars 50001)"
assert_exit_code 1 run_lf task cancel "$EDIT_ID" --reason "$REASON_LONG"

echo ""
echo "--- Test: Input Size Validation (HTTP API) ---"

# Start API server with master key auth
PORT=$(allocate_port)
BASE="http://127.0.0.1:$PORT/api/v1"
PBASE="$BASE/projects/1"
MASTER_KEY="test-key"

SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT

wait_for "API server ready" 10 "curl -sf $BASE/health >/dev/null"

TEST_TOKEN=$(create_test_user_key "http://127.0.0.1:$PORT" "$MASTER_KEY")

api_json() {
  curl -sf -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}
api_status() {
  curl -s -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" -H "Authorization: Bearer $TEST_TOKEN" "$@"
}

# ===== [10] API: title over limit → 400 =====

echo "[10] API: title over limit → 400"
STATUS=$(api_status -X POST "$PBASE/tasks" -d "{\"title\":\"$TITLE_501\"}")
assert_eq "400" "$STATUS" "oversized title returns 400"

# ===== [11] API: title at limit → 201 =====

echo "[11] API: title at limit → 201"
TASK=$(api_json -X POST "$PBASE/tasks" -d "{\"title\":\"$TITLE_500\"}")
assert_json_field "$TASK" '.status' "draft" "valid title accepted via API"

# ===== [12] API: too many tags → 400 =====

echo "[12] API: too many tags → 400"
TAGS_JSON="$(printf ',"%s"' $(seq -f 'tag%.0f' 1 21) | sed 's/^,//')"
STATUS=$(api_status -X POST "$PBASE/tasks" -d "{\"title\":\"Tag Overflow\",\"tags\":[$TAGS_JSON]}")
assert_eq "400" "$STATUS" "21 tags returns 400"

# ===== [13] API: edit title over limit → 400 =====

echo "[13] API: edit title over limit → 400"
TASK_API=$(api_json -X POST "$PBASE/tasks" -d '{"title":"API Edit Target"}')
TASK_API_ID=$(echo "$TASK_API" | jq -r '.id')
STATUS=$(api_status -X PUT "$PBASE/tasks/$TASK_API_ID" -d "{\"title\":\"$TITLE_501\"}")
assert_eq "400" "$STATUS" "oversized edit title returns 400"

# ===== [14] API: start with oversized session_id → 400 =====

echo "[14] API: start with oversized session_id → 400"
api_json -X POST "$PBASE/tasks/$TASK_API_ID/publish" -d '{}' >/dev/null
SESSION_101="$(gen_chars 101)"
STATUS=$(api_status -X POST "$PBASE/tasks/$TASK_API_ID/start" -d "{\"session_id\":\"$SESSION_101\"}")
assert_eq "400" "$STATUS" "oversized session_id returns 400"

test_summary
