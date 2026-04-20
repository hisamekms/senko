#!/usr/bin/env bash
# e2e test: --output text format validation for major commands

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Text Output Format ---"

# 1. add --output text
echo "[1] add text output"
ADD_TEXT="$(run_lf --output text task add --title "Text Test Task")"
assert_contains "$ADD_TEXT" "Created task #" "add text contains 'Created task #'"
assert_contains "$ADD_TEXT" "Text Test Task" "add text contains task title"
TASK_ID="$(echo "$ADD_TEXT" | grep -oP '#\K[0-9]+')"

# 2. list --output text
echo "[2] list text output"
LIST_TEXT="$(run_lf --output text task list)"
assert_contains "$LIST_TEXT" "#$TASK_ID" "list text contains task ID"
assert_contains "$LIST_TEXT" "Text Test Task" "list text contains task title"
assert_contains "$LIST_TEXT" "draft" "list text contains status"

# 3. publish --output text (transition to todo)
echo "[3] publish text output"
EDIT_TEXT="$(run_lf --output text task publish "$TASK_ID")"
assert_contains "$EDIT_TEXT" "Published task" "publish text contains 'Published task'"
assert_contains "$EDIT_TEXT" "$TASK_ID" "publish text contains task id"

# 4. next --output text
echo "[4] next text output"
NEXT_TEXT="$(run_lf --output text task next)"
assert_contains "$NEXT_TEXT" "Started task #$TASK_ID" "next text contains 'Started task #ID'"
assert_contains "$NEXT_TEXT" "Text Test Task" "next text contains task title"

# 5. complete --output text
echo "[5] complete text output"
COMP_TEXT="$(run_lf --output text task complete "$TASK_ID")"
assert_contains "$COMP_TEXT" "Completed task #$TASK_ID" "complete text contains 'Completed task #ID'"

# 6. cancel --output text
echo "[6] cancel text output"
CANC_ADD="$(run_lf --output json task add --title "Cancel Text Test")"
CANC_ID="$(echo "$CANC_ADD" | jq -r '.id')"
CANC_TEXT="$(run_lf --output text task cancel "$CANC_ID" --reason "test reason")"
assert_contains "$CANC_TEXT" "Canceled task #$CANC_ID" "cancel text contains 'Canceled task #ID'"
assert_contains "$CANC_TEXT" "test reason" "cancel text contains reason"

test_summary
