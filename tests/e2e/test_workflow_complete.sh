#!/usr/bin/env bash
# e2e tests for workflow-aware complete command
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Workflow Complete ---"

# Helper: create a task and move it to in_progress
create_in_progress_task() {
  local out
  out="$(run_lf add --title "$1")"
  local id
  id="$(echo "$out" | jq -r '.id')"
  run_lf ready "$id" > /dev/null
  run_lf start "$id" > /dev/null
  echo "$id"
}

echo "[1] default mode (merge_then_complete) completes without pr_url"
TASK_ID="$(create_in_progress_task "Default mode task")"
COMPLETE_OUT="$(run_lf complete "$TASK_ID")"
assert_json_field "$COMPLETE_OUT" '.status' "completed" "default mode completes"

echo "[2] pr_then_complete fails without pr_url"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[workflow]
completion_mode = "pr_then_complete"
EOF
TASK_ID2="$(create_in_progress_task "PR mode task")"
FAIL_OUT="$(run_lf complete "$TASK_ID2" 2>&1 || true)"
assert_contains "$FAIL_OUT" "pr_url" "error mentions pr_url"

echo "[3] --skip-pr-check bypasses pr_then_complete check"
TASK_ID3="$(create_in_progress_task "Skip check task")"
SKIP_OUT="$(run_lf complete "$TASK_ID3" --skip-pr-check)"
assert_json_field "$SKIP_OUT" '.status' "completed" "skip-pr-check completes"

echo "[4] pr_then_complete with pr_url but no gh fails gracefully"
TASK_ID4="$(create_in_progress_task "PR with url task")"
run_lf edit "$TASK_ID4" --pr-url "https://github.com/org/repo/pull/1" > /dev/null
# This will fail because gh is either not installed or the PR doesn't exist
# But it should give a clear error, not crash
FAIL_GH="$(run_lf complete "$TASK_ID4" 2>&1 || true)"
# Should mention gh or PR status
if [[ "$FAIL_GH" == *"gh"* ]] || [[ "$FAIL_GH" == *"PR"* ]] || [[ "$FAIL_GH" == *"error"* ]]; then
  echo "  PASS: gh failure gives clear error"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: expected gh-related error message"
  echo "    actual: $FAIL_GH"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[5] default mode ignores pr_url (completes even with pr_url set)"
rm "$TEST_PROJECT_ROOT/.senko/config.toml"
TASK_ID5="$(create_in_progress_task "Default with pr_url")"
run_lf edit "$TASK_ID5" --pr-url "https://github.com/org/repo/pull/99" > /dev/null
DEFAULT_OUT="$(run_lf complete "$TASK_ID5")"
assert_json_field "$DEFAULT_OUT" '.status' "completed" "default mode ignores pr_url"

test_summary
