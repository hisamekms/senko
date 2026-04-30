#!/usr/bin/env bash
# e2e test: `senko dev seed` (dev-tools feature)
#
# Verifies that the seeder loads the expected fixture sizes, that `reset`
# is idempotent across re-runs, and that `append` is a noop once the data
# has been seeded.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: dev seed ---"

# Expected fixture sizes (kept in sync with src/dev/seeder/fixtures.rs).
EXPECTED_TASKS=60
EXPECTED_CONTRACTS=5
EXPECTED_NOTES_C1=4   # contract index 0 (Auth refactor) has 4 notes

count_tasks() {
  run_lf --output json task list --limit 200 | jq -r '.items | length'
}

count_contracts() {
  run_lf --output json contract list --limit 50 | jq -r '.items | length'
}

count_notes() {
  local contract_id="$1"
  run_lf --output json contract note list "$contract_id" --limit 100 | jq -r '.items | length'
}

# 1. Reset on a fresh DB inserts the full fixture set
echo "[1] reset on a fresh DB"
run_lf dev seed reset >/dev/null
assert_eq "$EXPECTED_TASKS"     "$(count_tasks)"     "tasks count after reset"
assert_eq "$EXPECTED_CONTRACTS" "$(count_contracts)" "contracts count after reset"
assert_eq "$EXPECTED_NOTES_C1"  "$(count_notes 1)"   "notes for contract #1 after reset"

# Spot-check that statuses, dependencies and DoD checked-state survived save()
TASK1_JSON="$(run_lf --output json task get 1)"
assert_json_field "$TASK1_JSON" '.status'        "completed"             "task #1 ended at completed"
assert_json_field "$TASK1_JSON" '.completed_at'  "2026-04-20T15:30:00Z"  "task #1 completed_at is the seeded value"
assert_json_field "$TASK1_JSON" '.definition_of_done[0].checked' "true"  "task #1 DoD item 0 is checked"

TASK3_JSON="$(run_lf --output json task get 3)"
assert_json_field "$TASK3_JSON" '.status'        "in_progress"           "task #3 is in_progress"
TASK3_DEPS="$(echo "$TASK3_JSON" | jq -c '.dependencies')"
assert_eq "[2]" "$TASK3_DEPS" "task #3 depends on task #2"

# Tagging: every seeded task carries the `seed` marker tag
TAGGED_COUNT="$(run_lf --output json task list --tag seed --limit 200 | jq -r '.items | length')"
assert_eq "$EXPECTED_TASKS" "$TAGGED_COUNT" "every seeded task carries the seed tag"

# 2. Reset is idempotent — re-running yields the same counts
echo "[2] reset is idempotent"
run_lf dev seed reset >/dev/null
assert_eq "$EXPECTED_TASKS"     "$(count_tasks)"     "tasks count after 2nd reset"
assert_eq "$EXPECTED_CONTRACTS" "$(count_contracts)" "contracts count after 2nd reset"

# 3. append on an already-seeded DB is a noop
echo "[3] append on already-seeded DB is a noop"
APPEND_OUT="$(run_lf dev seed append 2>&1)"
assert_contains "$APPEND_OUT" "already seeded" "append reports noop"
assert_eq "$EXPECTED_TASKS" "$(count_tasks)" "tasks count unchanged after append"

# 4. After reset the default project (id=1) and default user (id=1) still exist
echo "[4] bootstrap rows survived reset"
PROJ_HAS_DEFAULT="$(run_lf --output json project list | jq -r '[.items[] | select(.id == 1)] | length')"
assert_eq "1" "$PROJ_HAS_DEFAULT" "default project id=1 still present after reset"
USER_HAS_DEFAULT="$(run_lf --output json user list | jq -r '[.items[] | select(.id == 1)] | length')"
assert_eq "1" "$USER_HAS_DEFAULT" "default user id=1 still present after reset"

# 5. append on a non-seeded DB seeds (covers the SQLite path; on Postgres
# the test database is shared per-worker so we exercise this only locally).
if [[ "${SENKO_TEST_BACKEND:-sqlite}" == "sqlite" ]]; then
  echo "[5] append on a fresh DB seeds"
  rm -f "$TEST_PROJECT_ROOT/.senko/data.db"
  run_lf dev seed append >/dev/null
  assert_eq "$EXPECTED_TASKS" "$(count_tasks)" "tasks count after append on fresh DB"
fi

test_summary
