#!/usr/bin/env bash
# e2e test: Edge cases and error handling

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Edge Cases ---"

# ===== [1] Non-existent task ID =====

echo "[1] Non-existent task ID"
assert_exit_code 1 run_lf --output json task get 99999
assert_exit_code 1 run_lf --output json task edit 99999 --title "X"
assert_exit_code 1 run_lf --output json task complete 99999
assert_exit_code 1 run_lf --output json task cancel 99999
assert_exit_code 1 run_lf --output json task deps list 99999

# ===== [2] Empty DB: list and next =====

echo "[2] Empty DB: list and next"
LIST_OUTPUT="$(run_lf --output json task list)"
assert_eq "[]" "$LIST_OUTPUT" "list on empty DB returns []"

NEXT_ERR="$(run_lf --output json task next 2>&1 || true)"
assert_contains "$NEXT_ERR" "no eligible task" "next on empty DB shows error"

# ===== [3] Duplicate tag via edit --add-tag (idempotent) =====

echo "[3] Duplicate tag via edit --add-tag"
ADD_OUT="$(run_lf --output json task add --title "Tag Test")"
TAG_ID="$(echo "$ADD_OUT" | jq -r '.id')"

run_lf task edit "$TAG_ID" --add-tag foo >/dev/null
OUT="$(run_lf --output json task edit "$TAG_ID" --add-tag foo)"
TAG_COUNT="$(echo "$OUT" | jq -r '.tags | length')"
assert_eq "1" "$TAG_COUNT" "add-tag foo twice results in 1 tag"

TAGS="$(echo "$OUT" | jq -r '.tags[0]')"
assert_eq "foo" "$TAGS" "tag is foo"

# add --tag with duplicate should fail (UNIQUE constraint)
echo "[3b] add --tag with duplicate values"
assert_exit_code 1 run_lf task add --title "Dup Tag" --tag a --tag a

# ===== [4] Duplicate dependency via deps add (idempotent) =====

echo "[4] Duplicate dependency via deps add"
A_OUT="$(run_lf --output json task add --title "Dep A")"
A_ID="$(echo "$A_OUT" | jq -r '.id')"
B_OUT="$(run_lf --output json task add --title "Dep B")"
B_ID="$(echo "$B_OUT" | jq -r '.id')"

run_lf task deps add "$A_ID" --on "$B_ID" >/dev/null
OUT="$(run_lf --output json task deps add "$A_ID" --on "$B_ID")"
DEP_COUNT="$(echo "$OUT" | jq -r '.dependencies | length')"
assert_eq "1" "$DEP_COUNT" "deps add twice results in 1 dependency"

# ===== [5] Complete from invalid status (draft / todo) =====

echo "[5] Complete from draft status"
DRAFT_OUT="$(run_lf --output json task add --title "Complete from Draft")"
DRAFT_ID="$(echo "$DRAFT_OUT" | jq -r '.id')"
assert_exit_code 1 run_lf task complete "$DRAFT_ID"

echo "[5b] Complete from todo status"
TODO_OUT="$(run_lf --output json task add --title "Complete from Todo")"
TODO_ID="$(echo "$TODO_OUT" | jq -r '.id')"
run_lf task publish "$TODO_ID" >/dev/null
assert_exit_code 1 run_lf task complete "$TODO_ID"

# ===== [6] Re-complete a completed task =====

echo "[6] Re-complete a completed task"
COMP_OUT="$(run_lf --output json task add --title "Complete Twice")"
COMP_ID="$(echo "$COMP_OUT" | jq -r '.id')"

run_lf task publish "$COMP_ID" >/dev/null
run_lf task start "$COMP_ID" >/dev/null
run_lf task complete "$COMP_ID" >/dev/null
assert_exit_code 1 run_lf task complete "$COMP_ID"

# ===== [7] Re-cancel a canceled task =====

echo "[7] Re-cancel a canceled task"
CANC_OUT="$(run_lf --output json task add --title "Cancel Twice")"
CANC_ID="$(echo "$CANC_OUT" | jq -r '.id')"

run_lf task cancel "$CANC_ID" >/dev/null
assert_exit_code 1 run_lf task cancel "$CANC_ID"

# ===== [8] Invalid priority value =====

echo "[8] Invalid priority value"
assert_exit_code 1 run_lf task add --title "Bad Priority" --priority p5
ERR_OUT="$(run_lf --output json task add --title "Bad Priority" --priority p5 2>&1 || true)"
assert_contains "$ERR_OUT" "invalid priority" "invalid priority error message"

# ===== [9] Invalid status filter in list =====

echo "[9] Invalid status filter in list"
assert_exit_code 1 run_lf task list --status blah
ERR_OUT="$(run_lf --output json task list --status blah 2>&1 || true)"
assert_contains "$ERR_OUT" "invalid" "invalid status filter error message"

# ===== [10] Invalid JSON input via --from-json =====

echo "[10] Invalid JSON input via --from-json"
INVALID_JSON_OUT="$(echo "not json" | run_lf --output json task add --from-json 2>&1 || true)"
assert_contains "$INVALID_JSON_OUT" "error" "malformed JSON returns error"

EMPTY_JSON_OUT="$(echo '{}' | run_lf --output json task add --from-json 2>&1 || true)"
assert_contains "$EMPTY_JSON_OUT" "error" "empty JSON object (missing title) returns error"

test_summary
