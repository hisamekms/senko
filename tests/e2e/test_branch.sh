#!/usr/bin/env bash
# e2e test: branch field on tasks

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Branch Field ---"

# 1. Add with --branch
echo "[1] Add with --branch"
OUT="$(run_lf --output json add --title "task with branch" --branch "feature/my-branch")"
TASK_ID="$(echo "$OUT" | jq -r '.id')"
assert_json_field "$OUT" '.branch' "feature/my-branch" "add --branch sets branch"

# 2. Get text output shows branch
echo "[2] Get text output shows branch"
OUT="$(run_lf --output text get "$TASK_ID")"
assert_contains "$OUT" "Branch:   feature/my-branch" "get text shows branch"

# 3. Edit --branch to change
echo "[3] Edit --branch"
OUT="$(run_lf --output json edit "$TASK_ID" --branch "feature/new-branch")"
assert_json_field "$OUT" '.branch' "feature/new-branch" "edit --branch changes branch"

# 4. Edit --clear-branch
echo "[4] Edit --clear-branch"
OUT="$(run_lf --output json edit "$TASK_ID" --clear-branch)"
assert_json_field "$OUT" '.branch' "null" "edit --clear-branch clears branch"

# 5. JSON output includes branch field
echo "[5] JSON output includes branch field"
OUT="$(run_lf --output json add --title "no branch task")"
assert_json_field "$OUT" '.branch' "null" "branch is null when not set"

# 6. ${task_id} template in add
echo "[6] \${task_id} template in add"
OUT="$(run_lf --output json add --title "template task" --branch 'task-${task_id}-feature')"
NEW_ID="$(echo "$OUT" | jq -r '.id')"
assert_json_field "$OUT" '.branch' "task-${NEW_ID}-feature" "add expands \${task_id} in branch"

# 7. ${task_id} template in edit
echo "[7] \${task_id} template in edit"
OUT="$(run_lf --output json edit "$TASK_ID" --branch 'work/${task_id}')"
assert_json_field "$OUT" '.branch' "work/${TASK_ID}" "edit expands \${task_id} in branch"

# 8. Add without --branch has null branch
echo "[8] Add without --branch"
OUT="$(run_lf --output json add --title "plain task")"
assert_json_field "$OUT" '.branch' "null" "branch is null by default"

test_summary
