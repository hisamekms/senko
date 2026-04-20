#!/usr/bin/env bash
# e2e test: metadata field validation on complete
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Metadata Complete Validation ---"

# Helper: create task and move to in_progress
create_in_progress_task() {
  local title="$1"
  shift
  local out
  out="$(run_lf task add --title "$title" "$@")"
  local id
  id="$(echo "$out" | jq -r '.id')"
  run_lf task publish "$id" > /dev/null
  run_lf task start "$id" > /dev/null
  echo "$id"
}

echo "[1] No metadata fields defined => complete works"
TASK_ID="$(create_in_progress_task "No fields task")"
COMPLETE_OUT="$(run_lf task complete "$TASK_ID")"
assert_json_field "$COMPLETE_OUT" '.status' "completed" "complete without fields"

echo "[2] Required field present => complete succeeds"
run_lf project metadata-field add --name sprint --type string --required-on-complete > /dev/null
TASK_ID2="$(create_in_progress_task "Has sprint" --metadata '{"sprint":"v1"}')"
COMPLETE_OUT2="$(run_lf task complete "$TASK_ID2")"
assert_json_field "$COMPLETE_OUT2" '.status' "completed" "complete with required field"

echo "[3] Required field missing => complete fails"
TASK_ID3="$(create_in_progress_task "Missing sprint")"
FAIL_OUT="$(run_lf task complete "$TASK_ID3" 2>&1 || true)"
assert_contains "$FAIL_OUT" "sprint" "error mentions missing field name"

echo "[4] Required field with wrong type => complete fails"
TASK_ID4="$(create_in_progress_task "Wrong type" --metadata '{"sprint":42}')"
FAIL_OUT2="$(run_lf task complete "$TASK_ID4" 2>&1 || true)"
assert_contains "$FAIL_OUT2" "sprint" "error mentions field with wrong type"
assert_contains "$FAIL_OUT2" "string" "error mentions expected type"

echo "[5] Multiple required fields, one missing => complete fails"
run_lf project metadata-field add --name points --type number --required-on-complete > /dev/null
TASK_ID5="$(create_in_progress_task "Partial meta" --metadata '{"sprint":"v2"}')"
FAIL_OUT3="$(run_lf task complete "$TASK_ID5" 2>&1 || true)"
assert_contains "$FAIL_OUT3" "points" "error mentions missing points field"

echo "[6] All required fields satisfied => complete succeeds"
TASK_ID6="$(create_in_progress_task "Full meta" --metadata '{"sprint":"v2","points":5}')"
COMPLETE_OUT3="$(run_lf task complete "$TASK_ID6")"
assert_json_field "$COMPLETE_OUT3" '.status' "completed" "complete with all required fields"

echo "[7] Non-required field absent => complete succeeds"
run_lf project metadata-field add --name notes --type string > /dev/null
TASK_ID7="$(create_in_progress_task "No notes" --metadata '{"sprint":"v3","points":3}')"
COMPLETE_OUT4="$(run_lf task complete "$TASK_ID7")"
assert_json_field "$COMPLETE_OUT4" '.status' "completed" "complete without optional field"

echo "[8] Boolean field type check"
run_lf project metadata-field add --name reviewed --type boolean --required-on-complete > /dev/null
TASK_ID8="$(create_in_progress_task "Bool wrong" --metadata '{"sprint":"v4","points":1,"reviewed":"yes"}')"
FAIL_OUT4="$(run_lf task complete "$TASK_ID8" 2>&1 || true)"
assert_contains "$FAIL_OUT4" "reviewed" "error mentions boolean field type mismatch"

echo "[9] Boolean field correct type => complete succeeds"
TASK_ID9="$(create_in_progress_task "Bool correct" --metadata '{"sprint":"v4","points":1,"reviewed":true}')"
COMPLETE_OUT5="$(run_lf task complete "$TASK_ID9")"
assert_json_field "$COMPLETE_OUT5" '.status' "completed" "complete with boolean field"

test_summary
