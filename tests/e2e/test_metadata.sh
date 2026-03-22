#!/usr/bin/env bash
# e2e test: metadata column (arbitrary JSON storage)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Metadata ---"

# [1] add with --metadata
echo "[1] add with --metadata"
ADD_OUT="$(run_lf add --title "Meta Task" --metadata '{"key":"value","num":42}')"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
assert_json_field "$ADD_OUT" '.metadata.key' "value" "add: metadata.key"
assert_json_field "$ADD_OUT" '.metadata.num' "42" "add: metadata.num"

# [2] get includes metadata
echo "[2] get includes metadata"
GET_OUT="$(run_lf get "$TASK_ID")"
assert_json_field "$GET_OUT" '.metadata.key' "value" "get: metadata.key"
assert_json_field "$GET_OUT" '.metadata.num' "42" "get: metadata.num"

# [3] get text output includes metadata
echo "[3] get text output includes metadata"
TEXT_OUT="$(run_lf --output text get "$TASK_ID")"
assert_contains "$TEXT_OUT" "Metadata:" "get text: contains Metadata label"

# [4] add without --metadata => metadata is null
echo "[4] add without --metadata => null"
ADD2_OUT="$(run_lf add --title "No Meta Task")"
TASK2_ID="$(echo "$ADD2_OUT" | jq -r '.id')"
assert_json_field "$ADD2_OUT" '.metadata' "null" "add: metadata is null"

# [5] edit --metadata to set metadata
echo "[5] edit --metadata"
run_lf edit "$TASK2_ID" --metadata '{"updated":true}'
GET2_OUT="$(run_lf get "$TASK2_ID")"
assert_json_field "$GET2_OUT" '.metadata.updated' "true" "edit: metadata.updated"

# [6] edit --clear-metadata
echo "[6] edit --clear-metadata"
run_lf edit "$TASK2_ID" --clear-metadata
GET3_OUT="$(run_lf get "$TASK2_ID")"
assert_json_field "$GET3_OUT" '.metadata' "null" "clear-metadata: metadata is null"

# [7] invalid JSON for --metadata on add
echo "[7] invalid JSON on add"
if run_lf add --title "Bad Meta" --metadata 'not json' 2>/dev/null; then
    echo "FAIL: should have failed with invalid JSON"
    exit 1
else
    echo "  PASS: invalid JSON rejected on add"
fi

# [8] invalid JSON for --metadata on edit
echo "[8] invalid JSON on edit"
if run_lf edit "$TASK_ID" --metadata 'also not json' 2>/dev/null; then
    echo "FAIL: should have failed with invalid JSON"
    exit 1
else
    echo "  PASS: invalid JSON rejected on edit"
fi

# [9] list includes metadata
echo "[9] list includes metadata"
LIST_OUT="$(run_lf list)"
HAS_META="$(echo "$LIST_OUT" | jq '.[0].metadata.key' -r)"
assert_eq "value" "$HAS_META" "list: first task has metadata.key"

# [10] --from-json with metadata
echo "[10] from-json with metadata"
FROM_JSON='{"title":"JSON Input","metadata":{"source":"api"}}'
FROM_OUT="$(echo "$FROM_JSON" | run_lf add --from-json)"
assert_json_field "$FROM_OUT" '.metadata.source' "api" "from-json: metadata.source"

# [11] edit --metadata replaces entire metadata
echo "[11] edit replaces entire metadata"
run_lf edit "$TASK_ID" --metadata '{"completely":"new"}'
GET4_OUT="$(run_lf get "$TASK_ID")"
assert_json_field "$GET4_OUT" '.metadata.completely' "new" "edit: replaced metadata"
# Old key should not exist
OLD_KEY="$(echo "$GET4_OUT" | jq -r '.metadata.key // "absent"')"
assert_eq "absent" "$OLD_KEY" "edit: old key removed"

echo ""
echo "All metadata tests passed!"
