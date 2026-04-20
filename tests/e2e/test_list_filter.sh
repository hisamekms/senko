#!/usr/bin/env bash
# e2e test: list subcommand filter options (--status, --tag, --depends-on, --ready, --contract, --id-min/--id-max, --limit/--offset)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: List Filter Options ---"

# Setup: Create tasks
# Task A: tags=backend,rust, status=todo
A_ID="$(run_lf --output json task add --title "Alpha" --tag backend --tag rust | jq -r '.id')"
run_lf task publish "$A_ID" >/dev/null

# Task B: tags=frontend, status=todo, depends on A
B_ID="$(run_lf --output json task add --title "Beta" --tag frontend | jq -r '.id')"
run_lf task publish "$B_ID" >/dev/null
run_lf task deps add "$B_ID" --on "$A_ID" >/dev/null

# Task C: tags=backend, status=completed
C_ID="$(run_lf --output json task add --title "Gamma" --tag backend | jq -r '.id')"
run_lf task publish "$C_ID" >/dev/null
run_lf task start "$C_ID" >/dev/null
run_lf task complete "$C_ID" >/dev/null

# --- Case 1: --status todo ---
echo "[1] --status todo filter"
LIST_TODO="$(run_lf --output json task list --status todo)"
TODO_COUNT="$(echo "$LIST_TODO" | jq 'length')"
TODO_HAS_A="$(echo "$LIST_TODO" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
TODO_HAS_B="$(echo "$LIST_TODO" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
TODO_HAS_C="$(echo "$LIST_TODO" | jq --arg id "$C_ID" '[.[] | select(.id == ($id | tonumber))] | length')"

assert_eq "2" "$TODO_COUNT" "status=todo returns 2 tasks"
assert_eq "1" "$TODO_HAS_A" "status=todo includes Alpha"
assert_eq "1" "$TODO_HAS_B" "status=todo includes Beta"
assert_eq "0" "$TODO_HAS_C" "status=todo excludes Gamma"

# --- Case 2: --tag backend ---
echo "[2] --tag backend filter"
LIST_BACKEND="$(run_lf --output json task list --tag backend)"
BACKEND_COUNT="$(echo "$LIST_BACKEND" | jq 'length')"
BACKEND_HAS_A="$(echo "$LIST_BACKEND" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
BACKEND_HAS_B="$(echo "$LIST_BACKEND" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
BACKEND_HAS_C="$(echo "$LIST_BACKEND" | jq --arg id "$C_ID" '[.[] | select(.id == ($id | tonumber))] | length')"

assert_eq "2" "$BACKEND_COUNT" "tag=backend returns 2 tasks"
assert_eq "1" "$BACKEND_HAS_A" "tag=backend includes Alpha"
assert_eq "0" "$BACKEND_HAS_B" "tag=backend excludes Beta"
assert_eq "1" "$BACKEND_HAS_C" "tag=backend includes Gamma"

# --- Case 3: --depends-on <Task A ID> ---
echo "[3] --depends-on filter"
LIST_DEPS="$(run_lf --output json task list --depends-on "$A_ID")"
DEPS_COUNT="$(echo "$LIST_DEPS" | jq 'length')"
DEPS_HAS_A="$(echo "$LIST_DEPS" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
DEPS_HAS_B="$(echo "$LIST_DEPS" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
DEPS_HAS_C="$(echo "$LIST_DEPS" | jq --arg id "$C_ID" '[.[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$DEPS_COUNT" "depends-on A returns 1 task"
assert_eq "0" "$DEPS_HAS_A" "depends-on A excludes Alpha"
assert_eq "1" "$DEPS_HAS_B" "depends-on A includes Beta"
assert_eq "0" "$DEPS_HAS_C" "depends-on A excludes Gamma"

# --- Case 4: --ready ---
echo "[4] --ready filter"
LIST_READY="$(run_lf --output json task list --ready)"
READY_HAS_A="$(echo "$LIST_READY" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
READY_HAS_B="$(echo "$LIST_READY" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$READY_HAS_A" "ready includes Alpha (no deps, todo)"
assert_eq "0" "$READY_HAS_B" "ready excludes Beta (unmet dep on Alpha)"

# --- Case 5: --contract <id> ---
echo "[5] --contract filter"
CONTRACT_ID="$(run_lf --output json contract add --title "ContractX" --definition-of-done "x" | jq -r '.id')"
D_ID="$(run_lf --output json task add --title "Delta" | jq -r '.id')"
run_lf task edit "$D_ID" --contract "$CONTRACT_ID" >/dev/null

LIST_CONTRACT="$(run_lf --output json task list --contract "$CONTRACT_ID")"
CONTRACT_COUNT="$(echo "$LIST_CONTRACT" | jq 'length')"
CONTRACT_HAS_D="$(echo "$LIST_CONTRACT" | jq --arg id "$D_ID" '[.[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$CONTRACT_COUNT" "--contract returns only linked task"
assert_eq "1" "$CONTRACT_HAS_D" "--contract includes Delta"

# --- Case 6: --id-min / --id-max ---
echo "[6] --id-min / --id-max filters"

# Bound by the full set of IDs created above (A, B, C, D)
LIST_ALL_IDS="$(run_lf --output json task list | jq -r '[.[].id] | sort | @csv')"
MIN_ID="$(run_lf --output json task list | jq '[.[].id] | min')"
MAX_ID="$(run_lf --output json task list | jq '[.[].id] | max')"

LIST_ID_MIN="$(run_lf --output json task list --id-min "$B_ID")"
ID_MIN_COUNT="$(echo "$LIST_ID_MIN" | jq 'length')"
ID_MIN_HAS_A="$(echo "$LIST_ID_MIN" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
ID_MIN_HAS_B="$(echo "$LIST_ID_MIN" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
assert_eq "0" "$ID_MIN_HAS_A" "--id-min excludes ids below threshold"
assert_eq "1" "$ID_MIN_HAS_B" "--id-min includes boundary id"

LIST_ID_MAX="$(run_lf --output json task list --id-max "$B_ID")"
ID_MAX_HAS_A="$(echo "$LIST_ID_MAX" | jq --arg id "$A_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
ID_MAX_HAS_B="$(echo "$LIST_ID_MAX" | jq --arg id "$B_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
ID_MAX_HAS_C="$(echo "$LIST_ID_MAX" | jq --arg id "$C_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$ID_MAX_HAS_A" "--id-max includes ids below threshold"
assert_eq "1" "$ID_MAX_HAS_B" "--id-max includes boundary id"
assert_eq "0" "$ID_MAX_HAS_C" "--id-max excludes ids above threshold"

LIST_RANGE="$(run_lf --output json task list --id-min "$B_ID" --id-max "$B_ID")"
RANGE_COUNT="$(echo "$LIST_RANGE" | jq 'length')"
assert_eq "1" "$RANGE_COUNT" "--id-min == --id-max returns exactly 1 task"

# --- Case 7: --limit / --offset ---
echo "[7] --limit / --offset pagination"

LIST_LIMIT2="$(run_lf --output json task list --limit 2)"
LIMIT_COUNT="$(echo "$LIST_LIMIT2" | jq 'length')"
assert_eq "2" "$LIMIT_COUNT" "--limit 2 returns exactly 2 tasks"

LIST_ALL="$(run_lf --output json task list --limit 200)"
TOTAL="$(echo "$LIST_ALL" | jq 'length')"
LIST_OFFSET="$(run_lf --output json task list --limit 200 --offset 2)"
OFFSET_COUNT="$(echo "$LIST_OFFSET" | jq 'length')"
EXPECTED_OFFSET=$((TOTAL - 2))
assert_eq "$EXPECTED_OFFSET" "$OFFSET_COUNT" "--offset 2 skips first 2 tasks"

# Default limit = 50 (we have only ~4 tasks, so all returned)
LIST_DEFAULT="$(run_lf --output json task list)"
DEFAULT_COUNT="$(echo "$LIST_DEFAULT" | jq 'length')"
assert_eq "$TOTAL" "$DEFAULT_COUNT" "default limit (50) returns all tasks when total < 50"

# --- Case 8: --limit validation ---
echo "[8] --limit validation"
if run_lf task list --limit 0 >/dev/null 2>&1; then
  echo "FAIL: --limit 0 should have errored"
  exit 1
else
  echo "PASS: --limit 0 rejected"
fi

if run_lf task list --limit 201 >/dev/null 2>&1; then
  echo "FAIL: --limit 201 should have errored"
  exit 1
else
  echo "PASS: --limit 201 rejected"
fi

test_summary
