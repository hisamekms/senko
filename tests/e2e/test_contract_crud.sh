#!/usr/bin/env bash
# e2e test: contract CRUD + list filter

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Contract CRUD ---"

# 1. Minimal add (title only) — verify defaults
echo "[1] Minimal contract (title only)"
ADD_MIN="$(run_lf --output json contract add --title "Minimal Contract")"

assert_json_field "$ADD_MIN" '.title' "Minimal Contract" "minimal: title"
assert_json_field "$ADD_MIN" '.description' "null" "minimal: description is null"
assert_json_field "$ADD_MIN" '.metadata' "null" "minimal: metadata is null"
assert_json_field "$ADD_MIN" '.is_completed' "false" "minimal: is_completed is false"
assert_eq '[]' "$(echo "$ADD_MIN" | jq -c '.tags')" "minimal: tags is empty array"
assert_eq '[]' "$(echo "$ADD_MIN" | jq -c '.definition_of_done')" "minimal: DoD is empty array"
assert_eq '[]' "$(echo "$ADD_MIN" | jq -c '.notes')" "minimal: notes is empty array"

# 2. Full add (all fields)
echo "[2] Full contract (all fields)"
ADD_FULL="$(run_lf --output json contract add \
  --title "Full Contract" \
  --description "desc text" \
  --definition-of-done "dod1" --definition-of-done "dod2" \
  --tag "t1" --tag "t2" \
  --metadata '{"k":"v"}')"

assert_json_field "$ADD_FULL" '.title' "Full Contract" "full: title"
assert_json_field "$ADD_FULL" '.description' "desc text" "full: description"
assert_eq '["t1","t2"]' "$(echo "$ADD_FULL" | jq -c '.tags')" "full: tags"
assert_eq '["dod1","dod2"]' "$(echo "$ADD_FULL" | jq -c '[.definition_of_done[].content]')" "full: DoD contents"
assert_eq '[false,false]' "$(echo "$ADD_FULL" | jq -c '[.definition_of_done[].checked]')" "full: DoD all unchecked"
assert_json_field "$ADD_FULL" '.metadata.k' "v" "full: metadata.k"

FULL_ID="$(echo "$ADD_FULL" | jq -r '.id')"

# 3. add --from-json (stdin)
echo "[3] Add from JSON (stdin)"
ADD_JSON="$(echo '{"title":"From JSON","description":"json-desc","tags":["a","b"],"definition_of_done":["x","y"]}' \
  | run_lf --output json contract add --from-json)"

assert_json_field "$ADD_JSON" '.title' "From JSON" "from-json: title"
assert_json_field "$ADD_JSON" '.description' "json-desc" "from-json: description"
assert_eq '["a","b"]' "$(echo "$ADD_JSON" | jq -c '.tags')" "from-json: tags"
assert_eq '["x","y"]' "$(echo "$ADD_JSON" | jq -c '[.definition_of_done[].content]')" "from-json: DoD"

# 4. add --from-json-file
echo "[4] Add from JSON file"
JSON_FILE="$TEST_DIR/contract_input.json"
cat > "$JSON_FILE" <<'EOF'
{"title":"From File","tags":["file-tag"],"definition_of_done":["file-dod"]}
EOF
ADD_FILE="$(run_lf --output json contract add --from-json-file "$JSON_FILE")"

assert_json_field "$ADD_FILE" '.title' "From File" "from-file: title"
assert_eq '["file-tag"]' "$(echo "$ADD_FILE" | jq -c '.tags')" "from-file: tags"
assert_eq '["file-dod"]' "$(echo "$ADD_FILE" | jq -c '[.definition_of_done[].content]')" "from-file: DoD"

# 5. get
echo "[5] Get contract"
GOT="$(run_lf --output json contract get "$FULL_ID")"
assert_json_field "$GOT" '.id' "$FULL_ID" "get: id"
assert_json_field "$GOT" '.title' "Full Contract" "get: title"
assert_json_field "$GOT" '.is_completed' "false" "get: is_completed"
assert_eq '[]' "$(echo "$GOT" | jq -c '.notes')" "get: notes empty"

# 6. list (all)
echo "[6] List all contracts"
LIST_ALL="$(run_lf --output json contract list)"
LIST_COUNT="$(echo "$LIST_ALL" | jq '.items | length')"
assert_eq "4" "$LIST_COUNT" "list: 4 contracts created"

# 7. list --tag filter
echo "[7] List contracts with --tag filter"
LIST_T1="$(run_lf --output json contract list --tag t1)"
T1_COUNT="$(echo "$LIST_T1" | jq '.items | length')"
assert_eq "1" "$T1_COUNT" "list --tag t1: only Full Contract"
assert_json_field "$LIST_T1" '.items[0].title' "Full Contract" "list --tag t1: title matches"

LIST_FILE="$(run_lf --output json contract list --tag file-tag)"
FILE_COUNT="$(echo "$LIST_FILE" | jq '.items | length')"
assert_eq "1" "$FILE_COUNT" "list --tag file-tag: only From File"

LIST_NONE="$(run_lf --output json contract list --tag nonexistent)"
assert_eq "0" "$(echo "$LIST_NONE" | jq '.items | length')" "list --tag nonexistent: empty"

# 7b. cursor pagination
echo "[7b] contract list cursor pagination"
PAGE1="$(run_lf --output json contract list --limit 2)"
assert_eq "2" "$(echo "$PAGE1" | jq '.items | length')" "page1 has 2 contracts"
CURSOR="$(echo "$PAGE1" | jq -r '.next_cursor')"
if [[ "$CURSOR" == "null" ]]; then
  echo "FAIL: expected next_cursor on first page" >&2
  exit 1
fi
PAGE2="$(run_lf --output json contract list --limit 2 --after "$CURSOR")"
assert_eq "2" "$(echo "$PAGE2" | jq '.items | length')" "page2 has remaining 2 contracts"
assert_eq "null" "$(echo "$PAGE2" | jq -r '.next_cursor')" "page2 has no more"
assert_exit_code 1 run_lf --output json contract list --after not-a-valid-cursor

# 7c. --order desc returns ID descending (CLI default is also desc)
echo "[7c] contract list --order desc"
LIST_DESC="$(run_lf --output json contract list --order desc)"
DESC_IDS="$(echo "$LIST_DESC" | jq -r '.items[].id' | tr '\n' ',' | sed 's/,$//')"
DESC_IDS_SORTED="$(echo "$LIST_DESC" | jq -r '.items[].id' | sort -rn | tr '\n' ',' | sed 's/,$//')"
assert_eq "$DESC_IDS_SORTED" "$DESC_IDS" "contract list --order desc sorts by id desc"

LIST_DEFAULT="$(run_lf --output json contract list)"
DEFAULT_IDS="$(echo "$LIST_DEFAULT" | jq -r '.items[].id' | tr '\n' ',' | sed 's/,$//')"
assert_eq "$DESC_IDS" "$DEFAULT_IDS" "contract list CLI default is desc"

# 7d. --order-by updated_at returns the touched contract first under desc
echo "[7d] contract list --order-by updated_at --order desc"
# updated_at is stored at second resolution; sleep so the touch lands in a
# later bucket than the most recent prior contract creation.
sleep 2
run_lf contract edit "$FULL_ID" --title "Full Contract" >/dev/null
LIST_RECENT="$(run_lf --output json contract list --order-by updated_at --order desc)"
RECENT_FIRST_ID="$(echo "$LIST_RECENT" | jq -r '.items[0].id')"
assert_eq "$FULL_ID" "$RECENT_FIRST_ID" "most recently updated contract appears first"

# 7e. cursor direction mismatch is rejected
echo "[7e] contract list cursor direction mismatch is rejected"
PAGE_DESC="$(run_lf --output json contract list --order desc --limit 1)"
CURSOR_DESC="$(echo "$PAGE_DESC" | jq -r '.next_cursor')"
if [[ "$CURSOR_DESC" == "null" || -z "$CURSOR_DESC" ]]; then
  echo "FAIL: expected non-null cursor under --order desc --limit 1"
  exit 1
fi
MISMATCH_OUT="$(run_lf contract list --order asc --after "$CURSOR_DESC" 2>&1 || true)"
if echo "$MISMATCH_OUT" | grep -qi "cursor does not match order"; then
  echo "PASS: cursor direction mismatch rejected"
else
  echo "FAIL: expected 'cursor does not match order' error"
  echo "$MISMATCH_OUT"
  exit 1
fi
assert_exit_code 1 run_lf contract list --order asc --after "$CURSOR_DESC"

# 8. edit scalar fields
echo "[8] Edit scalar fields"
OUT="$(run_lf --output json contract edit "$FULL_ID" --title "Renamed")"
assert_json_field "$OUT" '.title' "Renamed" "edit: title"

OUT="$(run_lf --output json contract edit "$FULL_ID" --description "new desc")"
assert_json_field "$OUT" '.description' "new desc" "edit: description"

OUT="$(run_lf --output json contract edit "$FULL_ID" --clear-description)"
assert_json_field "$OUT" '.description' "null" "edit: clear-description"

# 9. edit metadata (merge / replace / clear)
echo "[9] Edit metadata"
OUT="$(run_lf --output json contract edit "$FULL_ID" --metadata '{"new":"added"}')"
assert_json_field "$OUT" '.metadata.k' "v" "edit metadata merge: k preserved"
assert_json_field "$OUT" '.metadata.new' "added" "edit metadata merge: new added"

OUT="$(run_lf --output json contract edit "$FULL_ID" --replace-metadata '{"only":"this"}')"
assert_json_field "$OUT" '.metadata.k' "null" "edit replace-metadata: k gone"
assert_json_field "$OUT" '.metadata.only' "this" "edit replace-metadata: only set"

OUT="$(run_lf --output json contract edit "$FULL_ID" --clear-metadata)"
assert_json_field "$OUT" '.metadata' "null" "edit clear-metadata"

# 10. edit tags (add / remove / set)
echo "[10] Edit tags"
OUT="$(run_lf --output json contract edit "$FULL_ID" --add-tag extra)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "extra,t1,t2" "$TAGS" "edit: add-tag extra"

OUT="$(run_lf --output json contract edit "$FULL_ID" --remove-tag t1)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "extra,t2" "$TAGS" "edit: remove-tag t1"

OUT="$(run_lf --output json contract edit "$FULL_ID" --set-tags alpha beta)"
TAGS="$(echo "$OUT" | jq -r '.tags | sort | join(",")')"
assert_eq "alpha,beta" "$TAGS" "edit: set-tags alpha beta"

# 11. edit DoD (add / remove / set)
echo "[11] Edit DoD"
OUT="$(run_lf --output json contract edit "$FULL_ID" --add-definition-of-done "dod3")"
DODS="$(echo "$OUT" | jq -c '[.definition_of_done[].content]')"
assert_eq '["dod1","dod2","dod3"]' "$DODS" "edit: add-definition-of-done dod3"

OUT="$(run_lf --output json contract edit "$FULL_ID" --remove-definition-of-done "dod2")"
DODS="$(echo "$OUT" | jq -c '[.definition_of_done[].content]')"
assert_eq '["dod1","dod3"]' "$DODS" "edit: remove-definition-of-done dod2"

OUT="$(run_lf --output json contract edit "$FULL_ID" --set-definition-of-done "only")"
DODS="$(echo "$OUT" | jq -c '[.definition_of_done[].content]')"
assert_eq '["only"]' "$DODS" "edit: set-definition-of-done"

# 12. delete
echo "[12] Delete contract"
DEL_OUT="$(run_lf --output json contract delete "$FULL_ID")"
assert_json_field "$DEL_OUT" '.deleted' "true" "delete: deleted flag true"
assert_json_field "$DEL_OUT" '.id' "$FULL_ID" "delete: id matches"

# 13. get after delete fails
echo "[13] Get after delete returns error"
assert_exit_code 1 run_lf --output json contract get "$FULL_ID"

# 14. delete nonexistent fails
echo "[14] Delete nonexistent contract fails"
assert_exit_code 1 run_lf --output json contract delete 99999

test_summary
