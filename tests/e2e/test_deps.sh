#!/usr/bin/env bash
# e2e test: Dependency management (deps add/remove/set/list, cycle detection, next filtering)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Dependency Management ---"

# Create tasks for dependency tests
A_ID="$(run_lf --output json add --title "Task A" | jq -r '.id')"
B_ID="$(run_lf --output json add --title "Task B" | jq -r '.id')"
C_ID="$(run_lf --output json add --title "Task C" | jq -r '.id')"

# Set all to todo
run_lf edit "$A_ID" --status todo >/dev/null
run_lf edit "$B_ID" --status todo >/dev/null
run_lf edit "$C_ID" --status todo >/dev/null

# 1. deps add + deps list
echo "[1] deps add and deps list"
ADD_OUTPUT="$(run_lf --output json deps add "$A_ID" --on "$B_ID")"
assert_contains "$(echo "$ADD_OUTPUT" | jq -r '.dependencies | map(tostring) | join(",")')" "$B_ID" "A depends on B"

LIST_OUTPUT="$(run_lf --output json deps list "$A_ID")"
LIST_IDS="$(echo "$LIST_OUTPUT" | jq -r '.[].id')"
assert_contains "$LIST_IDS" "$B_ID" "deps list shows B"

# 2. deps remove
echo "[2] deps remove"
REMOVE_OUTPUT="$(run_lf --output json deps remove "$A_ID" --on "$B_ID")"
REMOVE_DEPS="$(echo "$REMOVE_OUTPUT" | jq -r '.dependencies | length')"
assert_eq "0" "$REMOVE_DEPS" "A has no dependencies after remove"

REMOVE_LIST="$(run_lf --output json deps list "$A_ID")"
REMOVE_LIST_LEN="$(echo "$REMOVE_LIST" | jq -r 'length')"
assert_eq "0" "$REMOVE_LIST_LEN" "deps list is empty after remove"

# 3. deps set (bulk replace)
echo "[3] deps set"
SET_OUTPUT="$(run_lf --output json deps set "$A_ID" --on "$B_ID" "$C_ID")"
SET_DEPS_LEN="$(echo "$SET_OUTPUT" | jq -r '.dependencies | length')"
assert_eq "2" "$SET_DEPS_LEN" "A has 2 dependencies after set"

SET_DEPS="$(echo "$SET_OUTPUT" | jq -r '.dependencies | sort | map(tostring) | join(",")')"
EXPECTED_DEPS="$(echo -e "$B_ID\n$C_ID" | sort | paste -sd ',')"
assert_eq "$EXPECTED_DEPS" "$SET_DEPS" "deps set replaced with B and C"

# Clear deps for next tests
run_lf deps set "$A_ID" --on >/dev/null 2>&1 || true

# 4. Circular dependency: A→B→C→A
echo "[4] Circular dependency detection"
run_lf deps add "$A_ID" --on "$B_ID" >/dev/null
run_lf deps add "$B_ID" --on "$C_ID" >/dev/null

CYCLE_OUTPUT="$(run_lf deps add "$C_ID" --on "$A_ID" 2>&1 || true)"
assert_contains "$CYCLE_OUTPUT" "cycle" "cycle detected for C→A"

# 5. Self-dependency
echo "[5] Self-dependency detection"
SELF_OUTPUT="$(run_lf deps add "$A_ID" --on "$A_ID" 2>&1 || true)"
assert_contains "$SELF_OUTPUT" "itself" "self-dependency error"

# 6. next respects dependencies (unmet deps → task not returned)
echo "[6] next skips tasks with unmet dependencies"

# Create fresh tasks for next-with-deps test
D_ID="$(run_lf --output json add --title "Dep Target" | jq -r '.id')"
E_ID="$(run_lf --output json add --title "Has Dep" | jq -r '.id')"

run_lf edit "$D_ID" --status todo >/dev/null
run_lf edit "$E_ID" --status todo >/dev/null

# E depends on D (D is not completed yet)
run_lf deps add "$E_ID" --on "$D_ID" >/dev/null

# Next should return D (not E, since E's dep is unmet)
NEXT_OUTPUT="$(run_lf --output json next)"
NEXT_ID="$(echo "$NEXT_OUTPUT" | jq -r '.id')"

if [[ "$NEXT_ID" == "$E_ID" ]]; then
  echo "  FAIL: next returned task with unmet dependency"
  FAIL_COUNT=$((FAIL_COUNT + 1))
else
  echo "  PASS: next did not return task with unmet dependency (returned #$NEXT_ID)"
  PASS_COUNT=$((PASS_COUNT + 1))
fi

# Complete D (the task next picked), then E should become available
run_lf complete "$NEXT_ID" >/dev/null

# If next didn't pick D, handle remaining tasks until D is completed
if [[ "$NEXT_ID" != "$D_ID" ]]; then
  # Complete other tasks until D is picked and completed
  while true; do
    NEXT2="$(run_lf --output json next)"
    NEXT2_ID="$(echo "$NEXT2" | jq -r '.id')"
    run_lf complete "$NEXT2_ID" >/dev/null
    if [[ "$NEXT2_ID" == "$D_ID" ]]; then
      break
    fi
  done
fi

# Now D is completed, E's dependency is met
NEXT_E="$(run_lf --output json next)"
NEXT_E_ID="$(echo "$NEXT_E" | jq -r '.id')"
assert_eq "$E_ID" "$NEXT_E_ID" "next returns task after its dependency is completed"

test_summary
