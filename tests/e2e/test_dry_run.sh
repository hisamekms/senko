#!/usr/bin/env bash
# e2e test: --dry-run global option

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Dry Run ---"

# 1. dry-run add (JSON output)
echo "[1] dry-run add (JSON)"
DR_ADD="$(run_lf --dry-run --output json task add --title "test task" --priority p1 --tag rust)"
assert_json_field "$DR_ADD" '.command' "add" "dry-run add command field"
assert_contains "$DR_ADD" "Create task with title" "dry-run add has create operation"
assert_contains "$DR_ADD" "P1" "dry-run add shows priority"
assert_contains "$DR_ADD" "rust" "dry-run add shows tags"

# 2. dry-run add does not actually create a task
echo "[2] dry-run add does not create task"
LIST_OUTPUT="$(run_lf --output json task list)"
TASK_COUNT="$(echo "$LIST_OUTPUT" | jq '.items | length')"
assert_eq "0" "$TASK_COUNT" "no tasks after dry-run add"

# 3. dry-run add (text output)
echo "[3] dry-run add (text)"
DR_ADD_TEXT="$(run_lf --dry-run --output text task add --title "test task")"
assert_contains "$DR_ADD_TEXT" "Create task with title" "dry-run text output shows operation"

# 4. dry-run complete with valid transition
echo "[4] dry-run complete"
ADD_OUT="$(run_lf --output json task add --title "Complete Me")"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
run_lf task publish "$TASK_ID" >/dev/null
run_lf task next >/dev/null

DR_COMPLETE="$(run_lf --dry-run --output json task complete "$TASK_ID")"
assert_json_field "$DR_COMPLETE" '.command' "complete" "dry-run complete command field"
assert_contains "$DR_COMPLETE" "in_progress" "dry-run complete shows current status"
assert_contains "$DR_COMPLETE" "completed" "dry-run complete shows target status"

# Verify status didn't change
GET_OUT="$(run_lf --output json task get "$TASK_ID")"
assert_json_field "$GET_OUT" '.status' "in_progress" "status unchanged after dry-run complete"

# 5. dry-run complete with invalid transition (should error)
echo "[5] dry-run complete invalid transition"
ADD_OUT2="$(run_lf --output json task add --title "Draft Task")"
TASK2_ID="$(echo "$ADD_OUT2" | jq -r '.id')"
# Task is in draft state, cannot complete directly
if run_lf --dry-run task complete "$TASK2_ID" 2>/dev/null; then
  echo "  FAIL: dry-run complete should error on invalid transition"
  FAIL_COUNT=$((FAIL_COUNT + 1))
else
  echo "  PASS: dry-run complete errors on invalid transition"
  PASS_COUNT=$((PASS_COUNT + 1))
fi

# 6. dry-run cancel
echo "[6] dry-run cancel"
ADD_OUT3="$(run_lf --output json task add --title "Cancel Me")"
TASK3_ID="$(echo "$ADD_OUT3" | jq -r '.id')"
run_lf task publish "$TASK3_ID" >/dev/null

DR_CANCEL="$(run_lf --dry-run --output json task cancel "$TASK3_ID" --reason "not needed")"
assert_json_field "$DR_CANCEL" '.command' "cancel" "dry-run cancel command field"
assert_contains "$DR_CANCEL" "canceled" "dry-run cancel shows target status"
assert_contains "$DR_CANCEL" "not needed" "dry-run cancel shows reason"

# Verify status didn't change
GET_OUT3="$(run_lf --output json task get "$TASK3_ID")"
assert_json_field "$GET_OUT3" '.status' "todo" "status unchanged after dry-run cancel"

# 7. dry-run next
echo "[7] dry-run next"
# Task3 is in todo, should be eligible for next
# But Task1 was started and is in_progress. Let's complete it first.
run_lf task complete "$TASK_ID" >/dev/null

DR_NEXT="$(run_lf --dry-run --output json task next)"
assert_json_field "$DR_NEXT" '.command' "next" "dry-run next command field"
assert_contains "$DR_NEXT" "Start next eligible task" "dry-run next shows candidate"

# Verify task status didn't change
GET_OUT3B="$(run_lf --output json task get "$TASK3_ID")"
assert_json_field "$GET_OUT3B" '.status' "todo" "status unchanged after dry-run next"

# 8. dry-run edit
echo "[8] dry-run edit"
DR_EDIT="$(run_lf --dry-run --output json task edit "$TASK3_ID" --title "New Title" --priority p0)"
assert_json_field "$DR_EDIT" '.command' "edit" "dry-run edit command field"
assert_contains "$DR_EDIT" "set title" "dry-run edit shows title change"
assert_contains "$DR_EDIT" "P0" "dry-run edit shows priority change"

# Verify task didn't change
GET_OUT3C="$(run_lf --output json task get "$TASK3_ID")"
assert_json_field "$GET_OUT3C" '.title' "Cancel Me" "title unchanged after dry-run edit"

# 9. dry-run deps add
echo "[9] dry-run deps add"
DR_DEPS="$(run_lf --dry-run --output json task deps add "$TASK3_ID" --on "$TASK2_ID")"
assert_json_field "$DR_DEPS" '.command' "deps add" "dry-run deps add command field"
assert_contains "$DR_DEPS" "depends on" "dry-run deps add shows dependency"

# 10. dry-run deps remove
echo "[10] dry-run deps remove"
DR_DEPS_RM="$(run_lf --dry-run --output json task deps remove "$TASK3_ID" --on "$TASK2_ID")"
assert_json_field "$DR_DEPS_RM" '.command' "deps remove" "dry-run deps remove command field"
assert_contains "$DR_DEPS_RM" "no longer depends" "dry-run deps remove shows removal"

# 11. dry-run deps set
echo "[11] dry-run deps set"
DR_DEPS_SET="$(run_lf --dry-run --output json task deps set "$TASK3_ID" --on "$TASK2_ID")"
assert_json_field "$DR_DEPS_SET" '.command' "deps set" "dry-run deps set command field"
assert_contains "$DR_DEPS_SET" "Set dependencies" "dry-run deps set shows operations"

# 12. dry-run skill-install
echo "[12] dry-run skill-install"
DR_SKILL="$(run_lf --dry-run --output json skill-install)"
assert_json_field "$DR_SKILL" '.command' "skill-install" "dry-run skill-install command field"
assert_contains "$DR_SKILL" "SKILL.md" "dry-run skill-install shows file write"

test_summary
