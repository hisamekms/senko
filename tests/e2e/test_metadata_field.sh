#!/usr/bin/env bash
# e2e test: Metadata field management (add, list, remove)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Metadata Field Management ---"

# [1] List empty (initially no fields)
echo "[1] List empty"
LIST_OUTPUT="$(run_lf project metadata-field list)"
FIELD_COUNT="$(echo "$LIST_OUTPUT" | jq '.items | length')"
assert_eq "0" "$FIELD_COUNT" "initially no metadata fields"

# [2] Add a string field
echo "[2] Add string field"
ADD_OUTPUT="$(run_lf project metadata-field add --name sprint --type string)"
assert_json_field "$ADD_OUTPUT" '.name' "sprint" "field name is sprint"
assert_json_field "$ADD_OUTPUT" '.field_type' "string" "field type is string"
assert_json_field "$ADD_OUTPUT" '.required_on_complete' "false" "not required by default"
assert_json_field "$ADD_OUTPUT" '.description' "null" "no description"

# [3] Add a number field with required_on_complete and description
echo "[3] Add number field with options"
ADD_OUTPUT="$(run_lf project metadata-field add --name points --type number --required-on-complete --description "Story points")"
assert_json_field "$ADD_OUTPUT" '.name' "points" "field name is points"
assert_json_field "$ADD_OUTPUT" '.field_type' "number" "field type is number"
assert_json_field "$ADD_OUTPUT" '.required_on_complete' "true" "required on complete"
assert_json_field "$ADD_OUTPUT" '.description' "Story points" "description set"

# [4] Add a boolean field
echo "[4] Add boolean field"
ADD_OUTPUT="$(run_lf project metadata-field add --name done --type boolean)"
assert_json_field "$ADD_OUTPUT" '.name' "done" "field name is done"
assert_json_field "$ADD_OUTPUT" '.field_type' "boolean" "field type is boolean"

# [5] List shows all three fields
echo "[5] List all fields"
LIST_OUTPUT="$(run_lf project metadata-field list)"
FIELD_COUNT="$(echo "$LIST_OUTPUT" | jq '.items | length')"
assert_eq "3" "$FIELD_COUNT" "three metadata fields"

# [6] Remove by name
echo "[6] Remove by name"
REMOVE_OUTPUT="$(run_lf project metadata-field remove --name sprint)"
assert_json_field "$REMOVE_OUTPUT" '.deleted' "sprint" "deleted field name"

# [7] List shows only remaining fields
echo "[7] List after removal"
LIST_OUTPUT="$(run_lf project metadata-field list)"
FIELD_COUNT="$(echo "$LIST_OUTPUT" | jq '.items | length')"
assert_eq "2" "$FIELD_COUNT" "two metadata fields after removal"
NAMES="$(echo "$LIST_OUTPUT" | jq -r '[.items[].name] | sort | join(",")')"
assert_eq "done,points" "$NAMES" "remaining fields are done and points"

# [8] Remove non-existent field (error)
echo "[8] Remove non-existent field"
assert_exit_code 1 run_lf project metadata-field remove --name nonexistent

# [9] Add duplicate name (error)
echo "[9] Add duplicate name"
assert_exit_code 1 run_lf project metadata-field add --name points --type string

# [10] Invalid field type (error)
echo "[10] Invalid field type"
assert_exit_code 1 run_lf project metadata-field add --name bad --type integer

# [11] Invalid field name (uppercase, error)
echo "[11] Invalid field name"
assert_exit_code 1 run_lf project metadata-field add --name Sprint --type string

# [12] Text output for list
echo "[12] Text output for list"
TEXT_LIST="$(run_lf --output text project metadata-field list)"
assert_contains "$TEXT_LIST" "points" "text list contains points"
assert_contains "$TEXT_LIST" "done" "text list contains done"
assert_contains "$TEXT_LIST" "[required]" "text list shows required marker"

# [13] Text output for add
echo "[13] Text output for add"
TEXT_ADD="$(run_lf --output text project metadata-field add --name status --type string)"
assert_contains "$TEXT_ADD" "Added metadata field" "text add confirmation"
assert_contains "$TEXT_ADD" "status" "text add shows name"

# [14] Text output for remove
echo "[14] Text output for remove"
TEXT_REMOVE="$(run_lf --output text project metadata-field remove --name status)"
assert_contains "$TEXT_REMOVE" "Removed metadata field" "text remove confirmation"
assert_contains "$TEXT_REMOVE" "status" "text remove shows name"

# [15] Cursor pagination round-trip
echo "[15] Cursor pagination round-trip"
# After all the prior steps we have: points + done = 2 fields.
PAGE1="$(run_lf --output json project metadata-field list --limit 1)"
assert_eq "1" "$(echo "$PAGE1" | jq '.items | length')" "page1 has 1 field"
CURSOR="$(echo "$PAGE1" | jq -r '.next_cursor')"
if [[ "$CURSOR" == "null" ]]; then
  echo "FAIL: expected next_cursor on first page" >&2
  exit 1
fi
PAGE2="$(run_lf --output json project metadata-field list --limit 1 --after "$CURSOR")"
assert_eq "1" "$(echo "$PAGE2" | jq '.items | length')" "page2 has 1 field"
assert_eq "null" "$(echo "$PAGE2" | jq -r '.next_cursor')" "page2 has no more"
assert_exit_code 1 run_lf --output json project metadata-field list --after bogus-cursor

test_summary
