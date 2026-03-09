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
assert_exit_code 1 run_lf --output json get 99999
assert_exit_code 1 run_lf --output json edit 99999 --title "X"
assert_exit_code 1 run_lf --output json complete 99999
assert_exit_code 1 run_lf --output json cancel 99999
assert_exit_code 1 run_lf --output json deps list 99999

# ===== [2] Empty DB: list and next =====

echo "[2] Empty DB: list and next"
LIST_OUTPUT="$(run_lf --output json list)"
assert_eq "[]" "$LIST_OUTPUT" "list on empty DB returns []"

NEXT_ERR="$(run_lf --output json next 2>&1 || true)"
assert_contains "$NEXT_ERR" "no eligible task" "next on empty DB shows error"

# ===== [3] Duplicate tag via edit --add-tag (idempotent) =====

echo "[3] Duplicate tag via edit --add-tag"
ADD_OUT="$(run_lf --output json add --title "Tag Test")"
TAG_ID="$(echo "$ADD_OUT" | jq -r '.id')"

run_lf edit "$TAG_ID" --add-tag foo >/dev/null
OUT="$(run_lf --output json edit "$TAG_ID" --add-tag foo)"
TAG_COUNT="$(echo "$OUT" | jq -r '.tags | length')"
assert_eq "1" "$TAG_COUNT" "add-tag foo twice results in 1 tag"

TAGS="$(echo "$OUT" | jq -r '.tags[0]')"
assert_eq "foo" "$TAGS" "tag is foo"

# add --tag with duplicate should fail (UNIQUE constraint)
echo "[3b] add --tag with duplicate values"
assert_exit_code 1 run_lf add --title "Dup Tag" --tag a --tag a

# ===== [4] Duplicate dependency via deps add (idempotent) =====

echo "[4] Duplicate dependency via deps add"
A_OUT="$(run_lf --output json add --title "Dep A")"
A_ID="$(echo "$A_OUT" | jq -r '.id')"
B_OUT="$(run_lf --output json add --title "Dep B")"
B_ID="$(echo "$B_OUT" | jq -r '.id')"

run_lf deps add "$A_ID" --on "$B_ID" >/dev/null
OUT="$(run_lf --output json deps add "$A_ID" --on "$B_ID")"
DEP_COUNT="$(echo "$OUT" | jq -r '.dependencies | length')"
assert_eq "1" "$DEP_COUNT" "deps add twice results in 1 dependency"

# ===== [5] Re-complete a completed task =====

echo "[5] Re-complete a completed task"
COMP_OUT="$(run_lf --output json add --title "Complete Twice")"
COMP_ID="$(echo "$COMP_OUT" | jq -r '.id')"

run_lf edit "$COMP_ID" --status todo >/dev/null
run_lf edit "$COMP_ID" --status in-progress >/dev/null
run_lf complete "$COMP_ID" >/dev/null
assert_exit_code 1 run_lf complete "$COMP_ID"

# ===== [6] Re-cancel a canceled task =====

echo "[6] Re-cancel a canceled task"
CANC_OUT="$(run_lf --output json add --title "Cancel Twice")"
CANC_ID="$(echo "$CANC_OUT" | jq -r '.id')"

run_lf cancel "$CANC_ID" >/dev/null
assert_exit_code 1 run_lf cancel "$CANC_ID"

test_summary
