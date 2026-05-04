#!/usr/bin/env bash
# e2e test: `senko task get` surfaces the assignee's username alongside the user_id
# in both text and JSON output. (See docs/proposals/show-username-in-task-display.md
# for the scope decision — only `task get` is converted in this task; other CLI
# commands keep raw Task JSON for now.)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

export SENKO_USER="alice"

echo "--- Test: assignee username in 'task get' output ---"

echo "[1] task add then task get JSON exposes assignee block (id/name)"
ADD_OUT="$(run_lf --output json task add --title "Has assignee" --assignee-user-id self)"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
GET_JSON="$(run_lf --output json task get "$TASK_ID")"
assert_json_field "$GET_JSON" '.assignee_user_id' "1" "task get JSON keeps assignee_user_id"
assert_json_field "$GET_JSON" '.assignee.id' "1" "task get JSON .assignee.id == user id"
assert_json_field "$GET_JSON" '.assignee.name' "alice" "task get JSON .assignee.name == username"

echo "[2] task get text output combines #<id> and username"
GET_TEXT="$(run_lf --output text task get "$TASK_ID")"
assert_contains "$GET_TEXT" "Assignee (user): #1 alice" "text output prints '#<uid> <username>'"

echo "[3] unassigned task → assignee block is null"
NA_OUT="$(run_lf --output json task add --title "No assignee")"
NA_ID="$(echo "$NA_OUT" | jq -r '.id')"
NA_GET="$(run_lf --output json task get "$NA_ID")"
assert_json_field "$NA_GET" '.assignee_user_id' "null" "no assignee → user_id null"
assert_json_field "$NA_GET" '.assignee' "null" "no assignee → assignee block null"

echo "[4] display_name passes through to assignee block"
DBOB_OUT="$(run_lf user create --username "bob" --display-name "Bob the Builder")"
DBOB_ID="$(echo "$DBOB_OUT" | jq -r '.id')"
DTASK_ADD="$(run_lf --output json task add --title "Bob's task" --assignee-user-id "$DBOB_ID")"
DTASK_ID="$(echo "$DTASK_ADD" | jq -r '.id')"
DGET="$(run_lf --output json task get "$DTASK_ID")"
assert_json_field "$DGET" '.assignee.name' "bob" "assignee.name = bob"
assert_json_field "$DGET" '.assignee.display_name' "Bob the Builder" "assignee.display_name passes through"

echo "[5] task get for a task with assignee_user_id whose user exists shows username"
EXTRA_OUT="$(run_lf user create --username "carol")"
EXTRA_ID="$(echo "$EXTRA_OUT" | jq -r '.id')"
ETASK_ADD="$(run_lf --output json task add --title "Carol task" --assignee-user-id "$EXTRA_ID")"
ETASK_ID="$(echo "$ETASK_ADD" | jq -r '.id')"
ETEXT="$(run_lf --output text task get "$ETASK_ID")"
assert_contains "$ETEXT" "Assignee (user): #${EXTRA_ID} carol" "text output for non-self assignee shows username"

# Note: the "deleted assignee user → null fallback" path cannot be exercised in
# e2e because `user delete` is blocked by SQLite's FOREIGN KEY constraint when
# the user is referenced by any task. That fallback is covered by the unit test
# `task_response_assignee_null_when_user_not_provided` in src/presentation/dto.rs.

test_summary
