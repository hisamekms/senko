#!/usr/bin/env bash
# e2e tests for pr_url field
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: PR URL ---"

echo "[1] pr_url defaults to null"
ADD_OUT="$(run_lf task add --title "Test task")"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
assert_json_field "$ADD_OUT" '.pr_url' "null" "pr_url defaults to null"

echo "[2] edit --pr-url sets pr_url"
EDIT_OUT="$(run_lf task edit "$TASK_ID" --pr-url "https://github.com/org/repo/pull/42")"
assert_json_field "$EDIT_OUT" '.pr_url' "https://github.com/org/repo/pull/42" "pr_url is set"

echo "[3] get returns pr_url"
GET_OUT="$(run_lf task get "$TASK_ID")"
assert_json_field "$GET_OUT" '.pr_url' "https://github.com/org/repo/pull/42" "get returns pr_url"

echo "[4] edit --clear-pr-url clears pr_url"
CLEAR_OUT="$(run_lf task edit "$TASK_ID" --clear-pr-url)"
assert_json_field "$CLEAR_OUT" '.pr_url' "null" "pr_url cleared to null"

echo "[5] text output shows pr_url"
run_lf task edit "$TASK_ID" --pr-url "https://github.com/org/repo/pull/99" > /dev/null
TEXT_OUT="$(run_lf --output text task get "$TASK_ID")"
assert_contains "$TEXT_OUT" "https://github.com/org/repo/pull/99" "text output contains pr_url"

echo "[6] text output omits pr_url when null"
run_lf task edit "$TASK_ID" --clear-pr-url > /dev/null
TEXT_OUT2="$(run_lf --output text task get "$TASK_ID")"
if [[ "$TEXT_OUT2" != *"PR URL"* ]]; then
  echo "  PASS: text output omits pr_url when null"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: text output should omit pr_url when null"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[7] list output includes pr_url"
run_lf task edit "$TASK_ID" --pr-url "https://github.com/org/repo/pull/1" > /dev/null
LIST_OUT="$(run_lf task list)"
LIST_PR="$(echo "$LIST_OUT" | jq -r '.items[0].pr_url')"
assert_eq "https://github.com/org/repo/pull/1" "$LIST_PR" "list includes pr_url"

echo "[8] dry-run shows pr_url operation"
DRY_OUT="$(run_lf --dry-run task edit "$TASK_ID" --pr-url "https://example.com/pr/2")"
assert_contains "$DRY_OUT" "pr_url" "dry-run mentions pr_url"

test_summary
