#!/usr/bin/env bash
# e2e test: contract notes (append-only, server-timestamped)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Contract Notes ---"

# Create a contract
ADD_OUT="$(run_lf --output json contract add --title "Note Contract")"
CID="$(echo "$ADD_OUT" | jq -r '.id')"

# 1. note add without source-task
echo "[1] note add without --source-task"
NOTE1="$(run_lf --output json contract note add "$CID" --content "first note")"
assert_json_field "$NOTE1" '.content' "first note" "note1: content"
assert_json_field "$NOTE1" '.source_task_id' "null" "note1: source_task_id is null"

CREATED_AT_1="$(echo "$NOTE1" | jq -r '.created_at')"
if [[ "$CREATED_AT_1" =~ ^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
  echo "  PASS: note1: created_at is ISO8601 ($CREATED_AT_1)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: note1: created_at not ISO8601: $CREATED_AT_1"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 2. note add with --source-task
echo "[2] note add with --source-task"
TID="$(run_lf --output json task add --title "Source Task" | jq -r '.id')"
NOTE2="$(run_lf --output json contract note add "$CID" --content "second note" --source-task "$TID")"
assert_json_field "$NOTE2" '.content' "second note" "note2: content"
assert_json_field "$NOTE2" '.source_task_id' "$TID" "note2: source_task_id matches task"

# 3. note list returns notes in insertion order (response is {items, next_cursor})
echo "[3] note list returns both notes in order"
NOTES="$(run_lf --output json contract note list "$CID")"
assert_eq "2" "$(echo "$NOTES" | jq '.items | length')" "list: 2 notes"
assert_json_field "$NOTES" '.items[0].content' "first note" "list[0]: first note"
assert_json_field "$NOTES" '.items[1].content' "second note" "list[1]: second note"
assert_json_field "$NOTES" '.items[0].source_task_id' "null" "list[0]: source_task_id null"
assert_json_field "$NOTES" '.items[1].source_task_id' "$TID" "list[1]: source_task_id matches"

# 3b. cursor pagination for notes
echo "[3b] note list cursor pagination"
PAGE1="$(run_lf --output json contract note list "$CID" --limit 1)"
assert_eq "1" "$(echo "$PAGE1" | jq '.items | length')" "page1 has 1 note"
CURSOR="$(echo "$PAGE1" | jq -r '.next_cursor')"
if [[ "$CURSOR" == "null" ]]; then
  echo "FAIL: expected next_cursor on first page" >&2
  exit 1
fi
PAGE2="$(run_lf --output json contract note list "$CID" --limit 1 --after "$CURSOR")"
assert_eq "1" "$(echo "$PAGE2" | jq '.items | length')" "page2 has remaining 1 note"
assert_eq "null" "$(echo "$PAGE2" | jq -r '.next_cursor')" "page2 has no more"
assert_exit_code 1 run_lf --output json contract note list "$CID" --after bogus-cursor

# 4. contract get embeds notes
echo "[4] contract get embeds notes"
GET_OUT="$(run_lf --output json contract get "$CID")"
assert_eq "2" "$(echo "$GET_OUT" | jq '.notes | length')" "get: 2 notes embedded"
assert_json_field "$GET_OUT" '.notes[0].content' "first note" "get.notes[0]: content"
assert_json_field "$GET_OUT" '.notes[1].content' "second note" "get.notes[1]: content"

# 5. nonexistent contract: note add fails
echo "[5] note add to nonexistent contract fails"
assert_exit_code 1 run_lf --output json contract note add 99999 --content "orphan"

# 6. nonexistent contract: note list fails
echo "[6] note list for nonexistent contract fails"
assert_exit_code 1 run_lf --output json contract note list 99999

test_summary
