#!/usr/bin/env bash
# e2e test: Project management (create, list, delete)
# project list response shape: { items: [...], next_cursor: <string|null> }

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Project Management ---"

# 1. Default project exists
echo "[1] Default project exists"
LIST_OUTPUT="$(run_lf project list)"
DEFAULT_COUNT="$(echo "$LIST_OUTPUT" | jq '[.items[] | select(.name == "default")] | length')"
assert_eq "1" "$DEFAULT_COUNT" "default project exists"

# 2. Create a project
echo "[2] Create project"
CREATE_OUTPUT="$(run_lf project create --name "test-project" --description "A test project")"
PROJECT_ID="$(echo "$CREATE_OUTPUT" | jq -r '.id')"
assert_json_field "$CREATE_OUTPUT" '.name' "test-project" "created project name"
assert_json_field "$CREATE_OUTPUT" '.description' "A test project" "created project description"

# 3. List includes new project
echo "[3] List includes new project"
LIST_OUTPUT="$(run_lf project list)"
PROJECT_COUNT="$(echo "$LIST_OUTPUT" | jq '.items | length')"
assert_eq "2" "$PROJECT_COUNT" "list has 2 projects"
FOUND="$(echo "$LIST_OUTPUT" | jq -r --arg id "$PROJECT_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$FOUND" "list contains created project"

# 4. Create project without description
echo "[4] Create project without description"
CREATE2_OUTPUT="$(run_lf project create --name "minimal-project")"
assert_json_field "$CREATE2_OUTPUT" '.name' "minimal-project" "minimal project name"
assert_json_field "$CREATE2_OUTPUT" '.description' "null" "minimal project description is null"

# 5. Delete project
echo "[5] Delete project"
DELETE_OUTPUT="$(run_lf project delete "$PROJECT_ID")"
LIST_OUTPUT="$(run_lf project list)"
REMAINING="$(echo "$LIST_OUTPUT" | jq -r --arg id "$PROJECT_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "0" "$REMAINING" "deleted project not in list"

# 6. Delete non-existent project (error)
echo "[6] Delete non-existent project"
assert_exit_code 1 run_lf project delete 9999

# 7. Text output
echo "[7] Text output"
TEXT_LIST="$(run_lf --output text project list)"
assert_contains "$TEXT_LIST" "default" "text list contains default project"

TEXT_CREATE="$(run_lf --output text project create --name "text-test")"
assert_contains "$TEXT_CREATE" "Created project" "text create output"

# 8. Cursor pagination round-trip: --limit + --after
echo "[8] Cursor pagination round-trip"
# After deleting PROJECT_ID we have: default + minimal-project + text-test = 3 projects.
PAGE1="$(run_lf --output json project list --limit 2)"
PAGE1_COUNT="$(echo "$PAGE1" | jq '.items | length')"
assert_eq "2" "$PAGE1_COUNT" "page1 has 2 items"
CURSOR="$(echo "$PAGE1" | jq -r '.next_cursor')"
if [[ "$CURSOR" == "null" ]]; then
  echo "FAIL: expected next_cursor on first page" >&2
  exit 1
fi
PAGE2="$(run_lf --output json project list --limit 2 --after "$CURSOR")"
PAGE2_COUNT="$(echo "$PAGE2" | jq '.items | length')"
assert_eq "1" "$PAGE2_COUNT" "page2 has remaining 1 item"
PAGE2_NEXT="$(echo "$PAGE2" | jq -r '.next_cursor')"
assert_eq "null" "$PAGE2_NEXT" "page2 has no more cursor"

# 9. Invalid cursor is rejected with a clear error
echo "[9] Invalid cursor rejected"
assert_exit_code 1 run_lf --output json project list --after not-a-valid-cursor

# 10. Text output includes trailing '... more: --after <cursor>' when more pages exist
echo "[10] Text output trailing cursor hint"
TEXT_PAGE1="$(run_lf --output text project list --limit 2)"
assert_contains "$TEXT_PAGE1" "... more: --after" "text shows cursor hint"

test_summary
