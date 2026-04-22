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
ADD_OUT="$(run_lf task add --title "Meta Task" --metadata '{"key":"value","num":42}')"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
assert_json_field "$ADD_OUT" '.metadata.key' "value" "add: metadata.key"
assert_json_field "$ADD_OUT" '.metadata.num' "42" "add: metadata.num"

# [2] get includes metadata
echo "[2] get includes metadata"
GET_OUT="$(run_lf task get "$TASK_ID")"
assert_json_field "$GET_OUT" '.metadata.key' "value" "get: metadata.key"
assert_json_field "$GET_OUT" '.metadata.num' "42" "get: metadata.num"

# [3] get text output includes metadata
echo "[3] get text output includes metadata"
TEXT_OUT="$(run_lf --output text task get "$TASK_ID")"
assert_contains "$TEXT_OUT" "Metadata:" "get text: contains Metadata label"

# [4] add without --metadata => metadata is null
echo "[4] add without --metadata => null"
ADD2_OUT="$(run_lf task add --title "No Meta Task")"
TASK2_ID="$(echo "$ADD2_OUT" | jq -r '.id')"
assert_json_field "$ADD2_OUT" '.metadata' "null" "add: metadata is null"

# [5] edit --metadata to set metadata
echo "[5] edit --metadata"
run_lf task edit "$TASK2_ID" --metadata '{"updated":true}'
GET2_OUT="$(run_lf task get "$TASK2_ID")"
assert_json_field "$GET2_OUT" '.metadata.updated' "true" "edit: metadata.updated"

# [6] edit --clear-metadata
echo "[6] edit --clear-metadata"
run_lf task edit "$TASK2_ID" --clear-metadata
GET3_OUT="$(run_lf task get "$TASK2_ID")"
assert_json_field "$GET3_OUT" '.metadata' "null" "clear-metadata: metadata is null"

# [7] invalid JSON for --metadata on add
echo "[7] invalid JSON on add"
if run_lf task add --title "Bad Meta" --metadata 'not json' 2>/dev/null; then
    echo "FAIL: should have failed with invalid JSON"
    exit 1
else
    echo "  PASS: invalid JSON rejected on add"
fi

# [8] invalid JSON for --metadata on edit
echo "[8] invalid JSON on edit"
if run_lf task edit "$TASK_ID" --metadata 'also not json' 2>/dev/null; then
    echo "FAIL: should have failed with invalid JSON"
    exit 1
else
    echo "  PASS: invalid JSON rejected on edit"
fi

# [9] list includes metadata
echo "[9] list includes metadata"
LIST_OUT="$(run_lf task list)"
HAS_META="$(echo "$LIST_OUT" | jq '.items[0].metadata.key' -r)"
assert_eq "value" "$HAS_META" "list: first task has metadata.key"

# [10] --from-json with metadata
echo "[10] from-json with metadata"
FROM_JSON='{"title":"JSON Input","metadata":{"source":"api"}}'
FROM_OUT="$(echo "$FROM_JSON" | run_lf task add --from-json)"
assert_json_field "$FROM_OUT" '.metadata.source' "api" "from-json: metadata.source"

# [11] edit --metadata shallow-merges metadata (preserves existing keys)
echo "[11] edit merges metadata"
run_lf task edit "$TASK_ID" --metadata '{"completely":"new"}'
GET4_OUT="$(run_lf task get "$TASK_ID")"
assert_json_field "$GET4_OUT" '.metadata.completely' "new" "edit: merged new key"
# Old key should still exist (shallow merge preserves unmentioned keys)
OLD_KEY="$(echo "$GET4_OUT" | jq -r '.metadata.key // "absent"')"
assert_eq "value" "$OLD_KEY" "edit: old key preserved"
OLD_NUM="$(echo "$GET4_OUT" | jq -r '.metadata.num // "absent"')"
assert_eq "42" "$OLD_NUM" "edit: old num preserved"

# [11b] edit --replace-metadata replaces entire metadata
echo "[11b] edit --replace-metadata replaces entire metadata"
run_lf task edit "$TASK_ID" --replace-metadata '{"only":"this"}'
GET4B_OUT="$(run_lf task get "$TASK_ID")"
assert_json_field "$GET4B_OUT" '.metadata.only' "this" "replace-metadata: new key"
OLD_KEY2="$(echo "$GET4B_OUT" | jq -r '.metadata.key // "absent"')"
assert_eq "absent" "$OLD_KEY2" "replace-metadata: old key removed"

# [11c] edit --metadata with null value deletes key
echo "[11c] edit --metadata with null deletes key"
run_lf task edit "$TASK_ID" --replace-metadata '{"a":1,"b":2,"c":3}'
run_lf task edit "$TASK_ID" --metadata '{"b":null}'
GET4C_OUT="$(run_lf task get "$TASK_ID")"
assert_json_field "$GET4C_OUT" '.metadata.a' "1" "null-delete: a preserved"
DELETED_KEY="$(echo "$GET4C_OUT" | jq -r '.metadata.b // "absent"')"
assert_eq "absent" "$DELETED_KEY" "null-delete: b removed"
assert_json_field "$GET4C_OUT" '.metadata.c' "3" "null-delete: c preserved"

# [11d] start --metadata merges into existing
echo "[11d] start --metadata merges"
MERGE_TASK="$(run_lf task add --title "Start Merge" --metadata '{"init":"val"}')"
MERGE_ID="$(echo "$MERGE_TASK" | jq -r '.id')"
run_lf task publish "$MERGE_ID"
STARTED="$(run_lf task start "$MERGE_ID" --metadata '{"added":"new"}')"
assert_json_field "$STARTED" '.metadata.init' "val" "start merge: existing key preserved"
assert_json_field "$STARTED" '.metadata.added' "new" "start merge: new key added"

# [12] metadata size limit on add (>64KB)
echo "[12] metadata size limit on add"
LARGE_META=$(python3 -c "import json; print(json.dumps({'big': 'x' * 70000}))")
if run_lf task add --title "Large Meta" --metadata "$LARGE_META" 2>/dev/null; then
    echo "FAIL: should reject metadata >64KB"
    exit 1
else
    echo "  PASS: large metadata rejected on add"
fi

# [13] metadata nesting depth limit on add (>10 levels)
echo "[13] metadata nesting depth limit on add"
DEEP_META='{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":1}}}}}}}}}}}'
if run_lf task add --title "Deep Meta" --metadata "$DEEP_META" 2>/dev/null; then
    echo "FAIL: should reject metadata with depth > 10"
    exit 1
else
    echo "  PASS: deeply nested metadata rejected on add"
fi

# [14] metadata at exactly the depth limit passes (10 levels)
echo "[14] metadata at exactly depth limit"
EXACT_META='{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":{"a":1}}}}}}}}}}'
EXACT_OUT="$(run_lf task add --title "Exact Depth" --metadata "$EXACT_META")"
assert_json_field "$EXACT_OUT" '.metadata.a.a.a.a.a.a.a.a.a.a' "1" "depth=10: accepted"

# [15] metadata size limit on edit (>64KB)
echo "[15] metadata size limit on edit"
if run_lf task edit "$TASK_ID" --metadata "$LARGE_META" 2>/dev/null; then
    echo "FAIL: should reject large metadata on edit"
    exit 1
else
    echo "  PASS: large metadata rejected on edit"
fi

echo ""
echo "All metadata tests passed!"
