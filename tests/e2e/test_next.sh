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

P3_ID="$(run_lf --output json add --title "P3 task" --priority p3 | jq -r '.id')"
P1_ID="$(run_lf --output json add --title "P1 task" --priority p1 | jq -r '.id')"
P2_ID="$(run_lf --output json add --title "P2 task" --priority p2 | jq -r '.id')"
P0_ID="$(run_lf --output json add --title "P0 task" --priority p0 | jq -r '.id')"

# Set all to todo
run_lf edit "$P3_ID" --status todo >/dev/null
run_lf edit "$P1_ID" --status todo >/dev/null
run_lf edit "$P2_ID" --status todo >/dev/null
run_lf edit "$P0_ID" --status todo >/dev/null

NEXT1="$(run_lf --output json next)"
NEXT1_ID="$(echo "$NEXT1" | jq -r '.id')"
assert_eq "$P0_ID" "$NEXT1_ID" "next picks P0 first"

NEXT2="$(run_lf --output json next)"
NEXT2_ID="$(echo "$NEXT2" | jq -r '.id')"
assert_eq "$P1_ID" "$NEXT2_ID" "next picks P1 second"

NEXT3="$(run_lf --output json next)"
NEXT3_ID="$(echo "$NEXT3" | jq -r '.id')"
assert_eq "$P2_ID" "$NEXT3_ID" "next picks P2 third"

NEXT4="$(run_lf --output json next)"
NEXT4_ID="$(echo "$NEXT4" | jq -r '.id')"
assert_eq "$P3_ID" "$NEXT4_ID" "next picks P3 last"

# Complete all in_progress tasks
run_lf complete "$P0_ID" >/dev/null
run_lf complete "$P1_ID" >/dev/null
run_lf complete "$P2_ID" >/dev/null
run_lf complete "$P3_ID" >/dev/null

# 2. Same priority: created_at/id ascending order
echo "[2] Same priority: earlier created task first"

FIRST_ID="$(run_lf --output json add --title "First same-pri" --priority p2 | jq -r '.id')"
SECOND_ID="$(run_lf --output json add --title "Second same-pri" --priority p2 | jq -r '.id')"

run_lf edit "$FIRST_ID" --status todo >/dev/null
run_lf edit "$SECOND_ID" --status todo >/dev/null

NEXT_SAME="$(run_lf --output json next)"
NEXT_SAME_ID="$(echo "$NEXT_SAME" | jq -r '.id')"
assert_eq "$FIRST_ID" "$NEXT_SAME_ID" "next picks earlier-created task first"

run_lf complete "$FIRST_ID" >/dev/null

NEXT_SAME2="$(run_lf --output json next)"
NEXT_SAME2_ID="$(echo "$NEXT_SAME2" | jq -r '.id')"
assert_eq "$SECOND_ID" "$NEXT_SAME2_ID" "next picks second task after first is completed"

run_lf complete "$SECOND_ID" >/dev/null

# 3. --session-id is recorded
echo "[3] --session-id is recorded"

SID_TASK_ID="$(run_lf --output json add --title "Session task" | jq -r '.id')"
run_lf edit "$SID_TASK_ID" --status todo >/dev/null

SID_OUTPUT="$(run_lf --output json next --session-id "test-session-42")"
SID_ACTUAL="$(echo "$SID_OUTPUT" | jq -r '.assignee_session_id')"
assert_eq "test-session-42" "$SID_ACTUAL" "session_id is recorded on task"

SID_STATUS="$(echo "$SID_OUTPUT" | jq -r '.status')"
assert_eq "in_progress" "$SID_STATUS" "task status changed to in_progress"

run_lf complete "$SID_TASK_ID" >/dev/null

# 4. Tasks with unmet dependencies are skipped
echo "[4] Dependency filtering: unmet deps skipped"

DEP_ID="$(run_lf --output json add --title "Dependency" --priority p2 | jq -r '.id')"
BLOCKED_ID="$(run_lf --output json add --title "Blocked task" --priority p0 | jq -r '.id')"

run_lf edit "$DEP_ID" --status todo >/dev/null
run_lf edit "$BLOCKED_ID" --status todo >/dev/null

# Blocked depends on Dep (Dep is not completed)
run_lf deps add "$BLOCKED_ID" --on "$DEP_ID" >/dev/null

# Even though Blocked has higher priority (p0), it should be skipped
NEXT_DEP="$(run_lf --output json next)"
NEXT_DEP_ID="$(echo "$NEXT_DEP" | jq -r '.id')"
assert_eq "$DEP_ID" "$NEXT_DEP_ID" "next skips task with unmet dependency (picks dep instead)"

run_lf complete "$DEP_ID" >/dev/null

# Now Blocked's dependency is met, it should be returned
NEXT_UNBLOCKED="$(run_lf --output json next)"
NEXT_UNBLOCKED_ID="$(echo "$NEXT_UNBLOCKED" | jq -r '.id')"
assert_eq "$BLOCKED_ID" "$NEXT_UNBLOCKED_ID" "next returns blocked task after dependency completed"

run_lf complete "$BLOCKED_ID" >/dev/null

# 5. No todo tasks: error exit
echo "[5] Error when no todo tasks"

# All tasks have been completed at this point
NEXT_EMPTY_OUTPUT="$(run_lf next 2>&1 || true)"
assert_contains "$NEXT_EMPTY_OUTPUT" "no eligible task" "error message when no todo tasks"

assert_exit_code 1 run_lf next

test_summary
