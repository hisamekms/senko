#!/usr/bin/env bash
# e2e test: next command priority control, session-id, dep filtering, empty case

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: next command ---"

# 1. Priority ordering: P0 > P1 > P2 > P3
echo "[1] Priority ordering P0 > P1 > P2 > P3"

P3_ID="$(run_lf --output json task add --title "P3 task" --priority p3 | jq -r '.id')"
P1_ID="$(run_lf --output json task add --title "P1 task" --priority p1 | jq -r '.id')"
P2_ID="$(run_lf --output json task add --title "P2 task" --priority p2 | jq -r '.id')"
P0_ID="$(run_lf --output json task add --title "P0 task" --priority p0 | jq -r '.id')"

# Set all to todo
run_lf task publish "$P3_ID" >/dev/null
run_lf task publish "$P1_ID" >/dev/null
run_lf task publish "$P2_ID" >/dev/null
run_lf task publish "$P0_ID" >/dev/null

NEXT1="$(run_lf --output json task next)"
NEXT1_ID="$(echo "$NEXT1" | jq -r '.id')"
assert_eq "$P0_ID" "$NEXT1_ID" "next picks P0 first"

NEXT2="$(run_lf --output json task next)"
NEXT2_ID="$(echo "$NEXT2" | jq -r '.id')"
assert_eq "$P1_ID" "$NEXT2_ID" "next picks P1 second"

NEXT3="$(run_lf --output json task next)"
NEXT3_ID="$(echo "$NEXT3" | jq -r '.id')"
assert_eq "$P2_ID" "$NEXT3_ID" "next picks P2 third"

NEXT4="$(run_lf --output json task next)"
NEXT4_ID="$(echo "$NEXT4" | jq -r '.id')"
assert_eq "$P3_ID" "$NEXT4_ID" "next picks P3 last"

# Complete all in_progress tasks
run_lf task complete "$P0_ID" >/dev/null
run_lf task complete "$P1_ID" >/dev/null
run_lf task complete "$P2_ID" >/dev/null
run_lf task complete "$P3_ID" >/dev/null

# 2. Same priority: created_at/id ascending order
echo "[2] Same priority: earlier created task first"

FIRST_ID="$(run_lf --output json task add --title "First same-pri" --priority p2 | jq -r '.id')"
SECOND_ID="$(run_lf --output json task add --title "Second same-pri" --priority p2 | jq -r '.id')"

run_lf task publish "$FIRST_ID" >/dev/null
run_lf task publish "$SECOND_ID" >/dev/null

NEXT_SAME="$(run_lf --output json task next)"
NEXT_SAME_ID="$(echo "$NEXT_SAME" | jq -r '.id')"
assert_eq "$FIRST_ID" "$NEXT_SAME_ID" "next picks earlier-created task first"

run_lf task complete "$FIRST_ID" >/dev/null

NEXT_SAME2="$(run_lf --output json task next)"
NEXT_SAME2_ID="$(echo "$NEXT_SAME2" | jq -r '.id')"
assert_eq "$SECOND_ID" "$NEXT_SAME2_ID" "next picks second task after first is completed"

run_lf task complete "$SECOND_ID" >/dev/null

# 3. --session-id is recorded
echo "[3] --session-id is recorded"

SID_TASK_ID="$(run_lf --output json task add --title "Session task" | jq -r '.id')"
run_lf task publish "$SID_TASK_ID" >/dev/null

SID_OUTPUT="$(run_lf --output json task next --session-id "test-session-42")"
SID_ACTUAL="$(echo "$SID_OUTPUT" | jq -r '.assignee_session_id')"
assert_eq "test-session-42" "$SID_ACTUAL" "session_id is recorded on task"

SID_STATUS="$(echo "$SID_OUTPUT" | jq -r '.status')"
assert_eq "in_progress" "$SID_STATUS" "task status changed to in_progress"

run_lf task complete "$SID_TASK_ID" >/dev/null

# 4. Tasks with unmet dependencies are skipped
echo "[4] Dependency filtering: unmet deps skipped"

DEP_ID="$(run_lf --output json task add --title "Dependency" --priority p2 | jq -r '.id')"
BLOCKED_ID="$(run_lf --output json task add --title "Blocked task" --priority p0 | jq -r '.id')"

run_lf task publish "$DEP_ID" >/dev/null
run_lf task publish "$BLOCKED_ID" >/dev/null

# Blocked depends on Dep (Dep is not completed)
run_lf task deps add "$BLOCKED_ID" --on "$DEP_ID" >/dev/null

# Even though Blocked has higher priority (p0), it should be skipped
NEXT_DEP="$(run_lf --output json task next)"
NEXT_DEP_ID="$(echo "$NEXT_DEP" | jq -r '.id')"
assert_eq "$DEP_ID" "$NEXT_DEP_ID" "next skips task with unmet dependency (picks dep instead)"

run_lf task complete "$DEP_ID" >/dev/null

# Now Blocked's dependency is met, it should be returned
NEXT_UNBLOCKED="$(run_lf --output json task next)"
NEXT_UNBLOCKED_ID="$(echo "$NEXT_UNBLOCKED" | jq -r '.id')"
assert_eq "$BLOCKED_ID" "$NEXT_UNBLOCKED_ID" "next returns blocked task after dependency completed"

run_lf task complete "$BLOCKED_ID" >/dev/null

# 5. No todo tasks: error exit (all completed)
echo "[5] Error when all tasks completed"

# All tasks have been completed at this point
NEXT_EMPTY_OUTPUT="$(run_lf task next 2>&1 || true)"
assert_contains "$NEXT_EMPTY_OUTPUT" "no eligible task" "error message when all tasks completed"

assert_exit_code 1 run_lf task next

# 6. All tasks in draft: next should fail
echo "[6] Error when all tasks in draft"

# Use a fresh environment for isolation
ORIG_DIR="$TEST_DIR"
setup_test_env

run_lf task add --title "Draft Only 1" >/dev/null
run_lf task add --title "Draft Only 2" >/dev/null

NEXT_DRAFT="$(run_lf task next 2>&1 || true)"
assert_contains "$NEXT_DRAFT" "no eligible task" "error when all tasks are draft"
assert_exit_code 1 run_lf task next

# 7. All todo tasks blocked by dependencies
echo "[7] Error when all todo tasks are blocked"

setup_test_env

BLOCKER_ID="$(run_lf --output json task add --title "Blocker" | jq -r '.id')"
BLOCKED1_ID="$(run_lf --output json task add --title "Blocked 1" | jq -r '.id')"
BLOCKED2_ID="$(run_lf --output json task add --title "Blocked 2" | jq -r '.id')"

run_lf task publish "$BLOCKED1_ID" >/dev/null
run_lf task publish "$BLOCKED2_ID" >/dev/null

run_lf task deps add "$BLOCKED1_ID" --on "$BLOCKER_ID" >/dev/null
run_lf task deps add "$BLOCKED2_ID" --on "$BLOCKER_ID" >/dev/null

# Blocker is still draft, so both todo tasks are blocked
NEXT_BLOCKED="$(run_lf task next 2>&1 || true)"
assert_contains "$NEXT_BLOCKED" "no eligible task" "error when all todo tasks are blocked by deps"
assert_exit_code 1 run_lf task next

test_summary
