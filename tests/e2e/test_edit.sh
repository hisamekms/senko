#!/usr/bin/env bash
# e2e test: edit subcommand (scalar fields, clear, array operations, error cases)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Edit Subcommand ---"

# Create a task to edit
ADD_OUTPUT="$(run_lf --output json add --title "Original Title")"
TASK_ID="$(echo "$ADD_OUTPUT" | jq -r '.id')"

# 1. Scalar field changes
echo "[1] Scalar field changes"

OUT="$(run_lf --output json edit "$TASK_ID" --title "New Title")"
assert_json_field "$OUT" '.title' "New Title" "edit title"

OUT="$(run_lf --output json edit "$TASK_ID" --background "bg text")"
assert_json_field "$OUT" '.background' "bg text" "edit background"

OUT="$(run_lf --output json edit "$TASK_ID" --details "detail text")"
assert_json_field "$OUT" '.details' "detail text" "edit details"

OUT="$(run_lf --output json edit "$TASK_ID" --priority p1)"
assert_json_field "$OUT" '.priority' "P1" "edit priority"

OUT="$(run_lf --output json edit "$TASK_ID" --status todo)"
assert_json_field "$OUT" '.status' "todo" "edit status"

# 2. Field clear
echo "[2] Field clear"

OUT="$(run_lf --output json edit "$TASK_ID" --clear-background)"
assert_json_field "$OUT" '.background' "null" "clear background"

OUT="$(run_lf --output json edit "$TASK_ID" --clear-details)"
assert_json_field "$OUT" '.details' "null" "clear details"

# 3. Array field operations (tags)
echo "[3] Array field operations (tags)"

OUT="$(run_lf --output json edit "$TASK_ID" --add-tag rust --add-tag cli)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "cli,rust" "$TAGS" "add-tag rust and cli"

OUT="$(run_lf --output json edit "$TASK_ID" --remove-tag cli)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "rust" "$TAGS" "remove-tag cli"

OUT="$(run_lf --output json edit "$TASK_ID" --set-tags new1 new2)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "new1,new2" "$TAGS" "set-tags replaces all"

# 4. Array field operations (definition_of_done)
echo "[4] Array field operations (definition_of_done)"

OUT="$(run_lf --output json edit "$TASK_ID" --add-definition-of-done "item1")"
DOD="$(echo "$OUT" | jq -r '[.definition_of_done[].content] | join(",")')"
assert_eq "item1" "$DOD" "add-definition-of-done item1"

OUT="$(run_lf --output json edit "$TASK_ID" --remove-definition-of-done "item1")"
DOD="$(echo "$OUT" | jq -r '.definition_of_done | length')"
assert_eq "0" "$DOD" "remove-definition-of-done item1"

# 4b. --set-definition-of-done (replace all DoD items)
echo "[4b] --set-definition-of-done (replace all)"

# First add some items
run_lf --output json edit "$TASK_ID" --add-definition-of-done "old1" --add-definition-of-done "old2" >/dev/null

# Replace all with --set-definition-of-done
OUT="$(run_lf --output json edit "$TASK_ID" --set-definition-of-done "new1" "new2" "new3")"
DOD="$(echo "$OUT" | jq -r '[.definition_of_done[].content] | join(",")')"
assert_eq "new1,new2,new3" "$DOD" "set-definition-of-done replaces all"

DOD_LEN="$(echo "$OUT" | jq -r '.definition_of_done | length')"
assert_eq "3" "$DOD_LEN" "set-definition-of-done count is 3"

# Verify all new items start unchecked
DOD_CHECKED="$(echo "$OUT" | jq -r '[.definition_of_done[].checked] | all(. == false)')"
assert_eq "true" "$DOD_CHECKED" "set-definition-of-done items are unchecked"

# Replace again with fewer items to confirm full replacement
OUT="$(run_lf --output json edit "$TASK_ID" --set-definition-of-done "only1")"
DOD="$(echo "$OUT" | jq -r '[.definition_of_done[].content] | join(",")')"
assert_eq "only1" "$DOD" "set-definition-of-done replaces to single item"

DOD_LEN="$(echo "$OUT" | jq -r '.definition_of_done | length')"
assert_eq "1" "$DOD_LEN" "set-definition-of-done count is 1 after replace"

# Clean up DoD for subsequent tests
run_lf --output json edit "$TASK_ID" --remove-definition-of-done "only1" >/dev/null

# 5. Non-existent task ID
echo "[5] Non-existent task ID"
assert_exit_code 1 run_lf --output json edit 9999 --title "X"

test_summary
