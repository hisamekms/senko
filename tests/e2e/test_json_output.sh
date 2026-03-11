#!/usr/bin/env bash
# e2e test: JSON output format validation for all commands

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: JSON Output ---"

# [1] add — JSON output structure
echo "[1] add — JSON output"
ADD_OUT="$(run_lf --output json add --title "JSON Test Task")"

# Verify it's valid JSON
echo "$ADD_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "add output is valid JSON"

# Verify fields and types
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.id | type == "number"')" "add: id is number"
assert_json_field "$ADD_OUT" '.title' "JSON Test Task" "add: title"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.title | type == "string"')" "add: title is string"
assert_json_field "$ADD_OUT" '.status' "draft" "add: status is draft"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.status | type == "string"')" "add: status is string"
assert_json_field "$ADD_OUT" '.priority' "P2" "add: default priority"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.priority | type == "string"')" "add: priority is string"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.tags | type == "array"')" "add: tags is array"
assert_json_field "$ADD_OUT" '.tags' "[]" "add: tags is empty"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.dependencies | type == "array"')" "add: dependencies is array"
assert_json_field "$ADD_OUT" '.dependencies' "[]" "add: dependencies is empty"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.created_at | type == "string"')" "add: created_at is string"
assert_eq "true" "$(echo "$ADD_OUT" | jq '.updated_at | type == "string"')" "add: updated_at is string"

# Optional fields should be null
assert_json_field "$ADD_OUT" '.background' "null" "add: background is null"
assert_json_field "$ADD_OUT" '.description' "null" "add: description is null"
assert_json_field "$ADD_OUT" '.plan' "null" "add: plan is null"

# [2] get — JSON output
echo "[2] get — JSON output"
GET_OUT="$(run_lf --output json get "$TASK_ID")"

echo "$GET_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "get output is valid JSON"

assert_json_field "$GET_OUT" '.id' "$TASK_ID" "get: id matches"
assert_json_field "$GET_OUT" '.title' "JSON Test Task" "get: title matches"
assert_json_field "$GET_OUT" '.status' "draft" "get: status matches"
assert_json_field "$GET_OUT" '.priority' "P2" "get: priority matches"

# [3] list — JSON output
echo "[3] list — JSON output"
# Add a second task
run_lf add --title "Second Task" >/dev/null
LIST_OUT="$(run_lf --output json list)"

echo "$LIST_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "list output is valid JSON"

LIST_TYPE="$(echo "$LIST_OUT" | jq 'type')"
assert_eq '"array"' "$LIST_TYPE" "list: output is array"

LIST_LEN="$(echo "$LIST_OUT" | jq 'length')"
assert_eq "true" "$(echo "$LIST_OUT" | jq 'length >= 2')" "list: contains at least 2 tasks"

# Verify our task is in the list
FOUND="$(echo "$LIST_OUT" | jq --argjson id "$TASK_ID" '[.[] | select(.id == $id)] | length')"
assert_eq "1" "$FOUND" "list: contains created task"

# [4] next — JSON output
echo "[4] next — JSON output"
# Move task to todo first
run_lf edit "$TASK_ID" --status todo >/dev/null
NEXT_OUT="$(run_lf --output json next)"

echo "$NEXT_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "next output is valid JSON"

assert_json_field "$NEXT_OUT" '.status' "in_progress" "next: status is in_progress"
STARTED_AT="$(echo "$NEXT_OUT" | jq -r '.started_at')"
assert_eq "false" "$([ "$STARTED_AT" = "null" ] && echo true || echo false)" "next: started_at is non-null"

# [5] complete — JSON output
echo "[5] complete — JSON output"
COMPLETE_OUT="$(run_lf --output json complete "$TASK_ID")"

echo "$COMPLETE_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "complete output is valid JSON"

assert_json_field "$COMPLETE_OUT" '.status' "completed" "complete: status is completed"
COMPLETED_AT="$(echo "$COMPLETE_OUT" | jq -r '.completed_at')"
assert_eq "false" "$([ "$COMPLETED_AT" = "null" ] && echo true || echo false)" "complete: completed_at is non-null"

# [6] cancel — JSON output
echo "[6] cancel — JSON output"
# Create a new task, move to todo, then cancel
CANCEL_ADD="$(run_lf --output json add --title "Cancel Target")"
CANCEL_ID="$(echo "$CANCEL_ADD" | jq -r '.id')"
run_lf edit "$CANCEL_ID" --status todo >/dev/null
CANCEL_OUT="$(run_lf --output json cancel "$CANCEL_ID" --reason "test cancel reason")"

echo "$CANCEL_OUT" | jq . >/dev/null 2>&1
assert_eq "0" "$?" "cancel output is valid JSON"

assert_json_field "$CANCEL_OUT" '.status' "canceled" "cancel: status is canceled"
assert_json_field "$CANCEL_OUT" '.cancel_reason' "test cancel reason" "cancel: cancel_reason is set"
CANCELED_AT="$(echo "$CANCEL_OUT" | jq -r '.canceled_at')"
assert_eq "false" "$([ "$CANCELED_AT" = "null" ] && echo true || echo false)" "cancel: canceled_at is non-null"

test_summary
