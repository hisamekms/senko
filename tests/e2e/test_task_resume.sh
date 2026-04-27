#!/usr/bin/env bash
# E2E tests for `senko task resume`: happy path, rejection cases, dry-run, HTTP route.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: task resume ---"

# Helper: create a task in a given status (mirrors test_status_transition.sh).
create_task_in_status() {
  local status="$1"
  local out id
  out="$(run_lf --output json task add --title "resume test $status")"
  id="$(echo "$out" | jq -r '.id')"
  case "$status" in
    draft) ;;
    todo)
      run_lf task publish "$id" >/dev/null
      ;;
    in_progress)
      run_lf task publish "$id" >/dev/null
      run_lf task start "$id" --session-id "sess-1" >/dev/null
      ;;
    completed)
      run_lf task publish "$id" >/dev/null
      run_lf task start "$id" >/dev/null
      run_lf task complete "$id" >/dev/null
      ;;
    canceled)
      run_lf task cancel "$id" >/dev/null
      ;;
  esac
  echo "$id"
}

echo "[1] Happy path: in_progress → resume updates session_id and metadata; status & started_at unchanged"
ID="$(create_task_in_status in_progress)"
BEFORE="$(run_lf --output json task get "$ID")"
START_AT_BEFORE="$(echo "$BEFORE" | jq -r '.started_at')"
OUT="$(run_lf --output json task resume "$ID" --session-id "sess-2" --metadata '{"phase":"recovery"}')"
assert_json_field "$OUT" '.status' "in_progress" "resume: status stays in_progress"
assert_json_field "$OUT" '.assignee_session_id' "sess-2" "resume: session_id refreshed"
assert_json_field "$OUT" '.metadata.phase' "recovery" "resume: metadata merged"
assert_json_field "$OUT" '.started_at' "$START_AT_BEFORE" "resume: started_at preserved"

echo "[2] Reject: draft → resume errors"
ID="$(create_task_in_status draft)"
assert_exit_code 1 run_lf task resume "$ID"

echo "[3] Reject: todo → resume errors"
ID="$(create_task_in_status todo)"
assert_exit_code 1 run_lf task resume "$ID"

echo "[4] Reject: completed → resume errors"
ID="$(create_task_in_status completed)"
assert_exit_code 1 run_lf task resume "$ID"

echo "[5] Reject: canceled → resume errors"
ID="$(create_task_in_status canceled)"
assert_exit_code 1 run_lf task resume "$ID"

echo "[6] dry-run: in_progress → resume prints Resume operation, makes no changes"
ID="$(create_task_in_status in_progress)"
DRY="$(run_lf --output json --dry-run task resume "$ID" --session-id "sess-9")"
assert_eq() { # local helper
  local expected="$1" actual="$2" msg="$3"
  if [[ "$expected" == "$actual" ]]; then
    echo "  PASS: $msg"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $msg (expected=$expected actual=$actual)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}
RESUME_OP="$(echo "$DRY" | jq -r '.operations[] | select(test("Resume task #"))')"
[[ -n "$RESUME_OP" ]] && {
  echo "  PASS: dry-run includes Resume operation"
  PASS_COUNT=$((PASS_COUNT + 1))
} || {
  echo "  FAIL: dry-run missing Resume operation; full output: $DRY"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}
AFTER="$(run_lf --output json task get "$ID")"
assert_json_field "$AFTER" '.assignee_session_id' "sess-1" "dry-run: session_id NOT changed"

echo "[7] HTTP route: POST /tasks/:id/resume"
PORT=$(allocate_port)
API_URL="http://127.0.0.1:$PORT"
MASTER_KEY=test-key
SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY" "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT
wait_for "API server ready" 10 "curl -sf $API_URL/api/v1/health >/dev/null"
TEST_TOKEN=$(create_test_user_key "$API_URL" "$MASTER_KEY")

run_http() {
  SENKO_CLI_REMOTE_URL="$API_URL" SENKO_CLI_REMOTE_TOKEN="$TEST_TOKEN" "$SENKO" --project-root "$TEST_PROJECT_ROOT" "$@"
}

T="$(run_http --output json task add --title "HTTP resume")"
HID="$(echo "$T" | jq -r '.id')"
run_http task publish "$HID" >/dev/null
run_http task start "$HID" --session-id "http-1" >/dev/null
RESUMED="$(run_http --output json task resume "$HID" --session-id "http-2" --metadata '{"k":"v"}')"
assert_json_field "$RESUMED" '.status' "in_progress" "HTTP resume: status stays in_progress"
assert_json_field "$RESUMED" '.assignee_session_id' "http-2" "HTTP resume: session refreshed"
assert_json_field "$RESUMED" '.metadata.k' "v" "HTTP resume: metadata merged"

echo "[8] HTTP reject: resume on todo task errors"
T="$(run_http --output json task add --title "HTTP resume reject")"
HID2="$(echo "$T" | jq -r '.id')"
run_http task publish "$HID2" >/dev/null
assert_exit_code 1 run_http task resume "$HID2"

test_summary
