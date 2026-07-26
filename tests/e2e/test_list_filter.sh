#!/usr/bin/env bash
# e2e test: list subcommand filter options (--status, --tag, --depends-on, --ready, --contract, --id-min/--id-max, --limit/--after)
#
# Response shape: { items: [...], next_cursor: <string|null> }

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
TODO_COUNT="$(echo "$LIST_TODO" | jq '.items | length')"
TODO_HAS_A="$(echo "$LIST_TODO" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
TODO_HAS_B="$(echo "$LIST_TODO" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
TODO_HAS_C="$(echo "$LIST_TODO" | jq --arg id "$C_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"

assert_eq "2" "$TODO_COUNT" "status=todo returns 2 tasks"
assert_eq "1" "$TODO_HAS_A" "status=todo includes Alpha"
assert_eq "1" "$TODO_HAS_B" "status=todo includes Beta"
assert_eq "0" "$TODO_HAS_C" "status=todo excludes Gamma"

# --- Case 2: --tag backend ---
echo "[2] --tag backend filter"
LIST_BACKEND="$(run_lf --output json task list --tag backend)"
BACKEND_COUNT="$(echo "$LIST_BACKEND" | jq '.items | length')"
BACKEND_HAS_A="$(echo "$LIST_BACKEND" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
BACKEND_HAS_B="$(echo "$LIST_BACKEND" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
BACKEND_HAS_C="$(echo "$LIST_BACKEND" | jq --arg id "$C_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"

assert_eq "2" "$BACKEND_COUNT" "tag=backend returns 2 tasks"
assert_eq "1" "$BACKEND_HAS_A" "tag=backend includes Alpha"
assert_eq "0" "$BACKEND_HAS_B" "tag=backend excludes Beta"
assert_eq "1" "$BACKEND_HAS_C" "tag=backend includes Gamma"

# --- Case 3: --depends-on <Task A ID> ---
echo "[3] --depends-on filter"
LIST_DEPS="$(run_lf --output json task list --depends-on "$A_ID")"
DEPS_COUNT="$(echo "$LIST_DEPS" | jq '.items | length')"
DEPS_HAS_A="$(echo "$LIST_DEPS" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
DEPS_HAS_B="$(echo "$LIST_DEPS" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
DEPS_HAS_C="$(echo "$LIST_DEPS" | jq --arg id "$C_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$DEPS_COUNT" "depends-on A returns 1 task"
assert_eq "0" "$DEPS_HAS_A" "depends-on A excludes Alpha"
assert_eq "1" "$DEPS_HAS_B" "depends-on A includes Beta"
assert_eq "0" "$DEPS_HAS_C" "depends-on A excludes Gamma"

# --- Case 4: --ready ---
echo "[4] --ready filter"
LIST_READY="$(run_lf --output json task list --ready)"
READY_HAS_A="$(echo "$LIST_READY" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
READY_HAS_B="$(echo "$LIST_READY" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$READY_HAS_A" "ready includes Alpha (no deps, todo)"
assert_eq "0" "$READY_HAS_B" "ready excludes Beta (unmet dep on Alpha)"

# --- Case 5: --contract <id> ---
echo "[5] --contract filter"
CONTRACT_ID="$(run_lf --output json contract add --title "ContractX" --definition-of-done "[manual] x" | jq -r '.id')"
D_ID="$(run_lf --output json task add --title "Delta" | jq -r '.id')"
run_lf task edit "$D_ID" --contract "$CONTRACT_ID" >/dev/null

LIST_CONTRACT="$(run_lf --output json task list --contract "$CONTRACT_ID")"
CONTRACT_COUNT="$(echo "$LIST_CONTRACT" | jq '.items | length')"
CONTRACT_HAS_D="$(echo "$LIST_CONTRACT" | jq --arg id "$D_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"

assert_eq "1" "$CONTRACT_COUNT" "--contract returns only linked task"
assert_eq "1" "$CONTRACT_HAS_D" "--contract includes Delta"

# --- Case 6: --id-min / --id-max ---
echo "[6] --id-min / --id-max filters"

LIST_ID_MIN="$(run_lf --output json task list --id-min "$B_ID")"
ID_MIN_HAS_A="$(echo "$LIST_ID_MIN" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
ID_MIN_HAS_B="$(echo "$LIST_ID_MIN" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "0" "$ID_MIN_HAS_A" "--id-min excludes ids below threshold"
assert_eq "1" "$ID_MIN_HAS_B" "--id-min includes boundary id"

LIST_ID_MAX="$(run_lf --output json task list --id-max "$B_ID")"
ID_MAX_HAS_A="$(echo "$LIST_ID_MAX" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
ID_MAX_HAS_B="$(echo "$LIST_ID_MAX" | jq --arg id "$B_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
ID_MAX_HAS_C="$(echo "$LIST_ID_MAX" | jq --arg id "$C_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$ID_MAX_HAS_A" "--id-max includes ids below threshold"
assert_eq "1" "$ID_MAX_HAS_B" "--id-max includes boundary id"
assert_eq "0" "$ID_MAX_HAS_C" "--id-max excludes ids above threshold"

LIST_RANGE="$(run_lf --output json task list --id-min "$B_ID" --id-max "$B_ID")"
RANGE_COUNT="$(echo "$LIST_RANGE" | jq '.items | length')"
assert_eq "1" "$RANGE_COUNT" "--id-min == --id-max returns exactly 1 task"

# --- Case 7: --limit / --after cursor pagination ---
echo "[7] --limit / --after cursor pagination"

# Total: A, B, C, D = 4 tasks (baseline for pagination)
LIST_ALL="$(run_lf --output json task list --limit 200)"
TOTAL="$(echo "$LIST_ALL" | jq '.items | length')"
ALL_CURSOR="$(echo "$LIST_ALL" | jq -r '.next_cursor')"
assert_eq "null" "$ALL_CURSOR" "fetching all items yields next_cursor=null"

# First page: --limit 2 → 2 items + non-null cursor
PAGE1="$(run_lf --output json task list --limit 2)"
PAGE1_COUNT="$(echo "$PAGE1" | jq '.items | length')"
PAGE1_CURSOR="$(echo "$PAGE1" | jq -r '.next_cursor')"
assert_eq "2" "$PAGE1_COUNT" "--limit 2 returns exactly 2 items"
if [[ "$PAGE1_CURSOR" == "null" ]]; then
  echo "FAIL: first page cursor should not be null"
  exit 1
fi
echo "PASS: first page has next_cursor"

# Second page: --after <cursor> --limit 2
PAGE2="$(run_lf --output json task list --limit 2 --after "$PAGE1_CURSOR")"
PAGE2_COUNT="$(echo "$PAGE2" | jq '.items | length')"
# Remaining after first page should be TOTAL - 2 (capped at 2 by limit)
EXPECTED_PAGE2=$((TOTAL - 2))
if (( EXPECTED_PAGE2 > 2 )); then EXPECTED_PAGE2=2; fi
assert_eq "$EXPECTED_PAGE2" "$PAGE2_COUNT" "--after returns the expected next batch"

# No page1/page2 id overlap
PAGE1_IDS="$(echo "$PAGE1" | jq -r '.items[].id' | sort -n | tr '\n' ',')"
PAGE2_IDS="$(echo "$PAGE2" | jq -r '.items[].id' | sort -n | tr '\n' ',')"
OVERLAP="$(comm -12 <(echo "$PAGE1" | jq -r '.items[].id' | sort -n) <(echo "$PAGE2" | jq -r '.items[].id' | sort -n) | wc -l | tr -d ' ')"
assert_eq "0" "$OVERLAP" "page1 and page2 share no ids (page1=$PAGE1_IDS page2=$PAGE2_IDS)"

# Exact-boundary case: fetch exactly TOTAL items in one page → next_cursor must be null
BOUNDARY="$(run_lf --output json task list --limit "$TOTAL")"
BOUNDARY_CURSOR="$(echo "$BOUNDARY" | jq -r '.next_cursor')"
BOUNDARY_COUNT="$(echo "$BOUNDARY" | jq '.items | length')"
assert_eq "$TOTAL" "$BOUNDARY_COUNT" "--limit == TOTAL returns all items"
assert_eq "null" "$BOUNDARY_CURSOR" "--limit == TOTAL yields next_cursor=null (no dangling more signal)"

# Default limit (50) with only ~4 tasks returns everything + null cursor
LIST_DEFAULT="$(run_lf --output json task list)"
DEFAULT_COUNT="$(echo "$LIST_DEFAULT" | jq '.items | length')"
DEFAULT_CURSOR="$(echo "$LIST_DEFAULT" | jq -r '.next_cursor')"
assert_eq "$TOTAL" "$DEFAULT_COUNT" "default limit (50) returns all tasks when total < 50"
assert_eq "null" "$DEFAULT_CURSOR" "default limit with total < 50 yields next_cursor=null"

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

# --- Case 9: invalid cursor ---
echo "[9] invalid --after cursor rejected"
if run_lf task list --after "not-a-valid-cursor!!!" >/dev/null 2>&1; then
  echo "FAIL: invalid --after cursor should have errored"
  exit 1
else
  echo "PASS: invalid --after cursor rejected"
fi

# --- Case 10: text output appends '... more: --after <cursor>' when more pages exist ---
echo "[10] --output text appends '... more:' when next_cursor is Some"
TEXT_OUT="$(run_lf --output text task list --limit 2)"
if echo "$TEXT_OUT" | grep -q "^... more: --after "; then
  echo "PASS: text output contains '... more: --after <cursor>'"
else
  echo "FAIL: text output did not contain '... more: --after <cursor>'"
  echo "---output---"
  echo "$TEXT_OUT"
  echo "------------"
  exit 1
fi

# When all items fit in one page, the '... more:' line must be absent
TEXT_OUT_FULL="$(run_lf --output text task list)"
if echo "$TEXT_OUT_FULL" | grep -q "^... more:"; then
  echo "FAIL: text output has '... more:' line when no more pages"
  exit 1
else
  echo "PASS: no '... more:' line when there's no next page"
fi

# --- Case 11: --order desc returns ID descending ---
echo "[11] task list --order desc returns ID descending"
LIST_DESC="$(run_lf --output json task list --order desc)"
DESC_IDS="$(echo "$LIST_DESC" | jq -r '.items[].id' | tr '\n' ',' | sed 's/,$//')"
DESC_IDS_SORTED="$(echo "$LIST_DESC" | jq -r '.items[].id' | sort -rn | tr '\n' ',' | sed 's/,$//')"
assert_eq "$DESC_IDS_SORTED" "$DESC_IDS" "--order desc returns items sorted by id desc"

# --- Case 12: CLI default is desc (no --order) ---
echo "[12] CLI default --order is desc"
LIST_DEFAULT_ORDER="$(run_lf --output json task list)"
DEFAULT_IDS="$(echo "$LIST_DEFAULT_ORDER" | jq -r '.items[].id' | tr '\n' ',' | sed 's/,$//')"
DEFAULT_IDS_SORTED="$(echo "$LIST_DEFAULT_ORDER" | jq -r '.items[].id' | sort -rn | tr '\n' ',' | sed 's/,$//')"
assert_eq "$DEFAULT_IDS_SORTED" "$DEFAULT_IDS" "no --order flag yields id desc (CLI default)"

# --- Case 13: --order-by updated_at --order desc returns updated_at desc, ties by id desc ---
echo "[13] --order-by updated_at --order desc"
# updated_at is stored at second resolution; sleep so the touch lands in a
# later bucket than the most recent prior edit.
sleep 2
run_lf task edit "$A_ID" --background "touch a" >/dev/null
LIST_RECENT="$(run_lf --output json task list --order-by updated_at --order desc)"
RECENT_FIRST_ID="$(echo "$LIST_RECENT" | jq -r '.items[0].id')"
assert_eq "$A_ID" "$RECENT_FIRST_ID" "task touched most recently appears first under updated_at desc"

# --- Case 14: cursor direction mismatch is rejected ---
echo "[14] cursor direction mismatch is rejected"
# Get a cursor under --order desc, then re-use it with --order asc.
PAGE_DESC="$(run_lf --output json task list --order desc --limit 1)"
CURSOR_DESC="$(echo "$PAGE_DESC" | jq -r '.next_cursor')"
if [[ "$CURSOR_DESC" == "null" || -z "$CURSOR_DESC" ]]; then
  echo "FAIL: expected non-null cursor under --order desc --limit 1"
  exit 1
fi
MISMATCH_OUT="$(run_lf task list --order asc --after "$CURSOR_DESC" 2>&1 || true)"
if echo "$MISMATCH_OUT" | grep -qi "cursor does not match order"; then
  echo "PASS: cursor direction mismatch rejected with CursorMismatch error"
else
  echo "FAIL: expected 'cursor does not match order' error"
  echo "---output---"
  echo "$MISMATCH_OUT"
  echo "------------"
  exit 1
fi
assert_exit_code 1 run_lf task list --order asc --after "$CURSOR_DESC"

# --- Case 15: cursor kind mismatch is rejected ---
echo "[15] cursor kind (order_by) mismatch is rejected"
# Cursor minted under order_by=id used with order_by=updated_at must mismatch.
PAGE_ID="$(run_lf --output json task list --order desc --limit 1)"
CURSOR_ID="$(echo "$PAGE_ID" | jq -r '.next_cursor')"
KIND_OUT="$(run_lf task list --order-by updated_at --order desc --after "$CURSOR_ID" 2>&1 || true)"
if echo "$KIND_OUT" | grep -qi "cursor does not match order_by"; then
  echo "PASS: cursor kind mismatch rejected with CursorMismatch error"
else
  echo "FAIL: expected 'cursor does not match order_by' error"
  echo "---output---"
  echo "$KIND_OUT"
  echo "------------"
  exit 1
fi

test_summary
