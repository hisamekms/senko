#!/usr/bin/env bash
# e2e test: assignee_user_id feature (add, edit, start, next, list --ready, from-json, backward compat)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

# === Section 1: add + assignee_user_id ===

export SENKO_USER="alice"

echo "--- Test: assignee_user_id ---"

echo "[1] add --assignee-user-id self sets assignee_user_id"
ADD_OUT="$(run_lf --output json task add --title "Assigned task" --assignee-user-id self)"
assert_json_field "$ADD_OUT" '.assignee_user_id' "1" "add with --assignee-user-id self sets user id 1"

echo "[2] add without --assignee-user-id leaves null"
ADD_OUT2="$(run_lf --output json task add --title "Unassigned task")"
assert_json_field "$ADD_OUT2" '.assignee_user_id' "null" "add without assignee leaves null"

echo "[3] add --assignee-user-id <numeric> sets specific user"
USER2_OUT="$(run_lf user create --username "bob")"
USER2_ID="$(echo "$USER2_OUT" | jq -r '.id')"
ADD_OUT3="$(run_lf --output json task add --title "Bob task" --assignee-user-id "$USER2_ID")"
assert_json_field "$ADD_OUT3" '.assignee_user_id' "$USER2_ID" "add with numeric assignee-user-id"

# === Section 2: edit + assignee_user_id ===

echo "[4] edit --assignee-user-id self sets assignee"
EDIT_TASK_ID="$(echo "$ADD_OUT2" | jq -r '.id')"
EDIT_OUT="$(run_lf --output json task edit "$EDIT_TASK_ID" --assignee-user-id self)"
assert_json_field "$EDIT_OUT" '.assignee_user_id' "1" "edit sets assignee-user-id via self"

echo "[5] edit --clear-assignee-user-id clears assignee"
EDIT_OUT2="$(run_lf --output json task edit "$EDIT_TASK_ID" --clear-assignee-user-id)"
assert_json_field "$EDIT_OUT2" '.assignee_user_id' "null" "edit clears assignee-user-id"

echo "[6] edit --assignee-user-id <numeric> sets specific user"
EDIT_OUT3="$(run_lf --output json task edit "$EDIT_TASK_ID" --assignee-user-id "$USER2_ID")"
assert_json_field "$EDIT_OUT3" '.assignee_user_id' "$USER2_ID" "edit sets numeric assignee-user-id"

# === Section 3: start + assignee validation ===

echo "[7] start auto-assigns unassigned task to current user"
AUTO_ID="$(run_lf --output json task add --title "Auto-assign" | jq -r '.id')"
run_lf task publish "$AUTO_ID" >/dev/null
START_OUT="$(run_lf --output json task start "$AUTO_ID")"
assert_json_field "$START_OUT" '.assignee_user_id' "1" "start auto-assigns to current user"
assert_json_field "$START_OUT" '.status' "in_progress" "start transitions to in_progress"

echo "[8] start succeeds on self-assigned task"
SELF_ID="$(run_lf --output json task add --title "Self-assigned" --assignee-user-id self | jq -r '.id')"
run_lf task publish "$SELF_ID" >/dev/null
START_OUT2="$(run_lf --output json task start "$SELF_ID")"
assert_json_field "$START_OUT2" '.status' "in_progress" "start self-assigned succeeds"
assert_json_field "$START_OUT2" '.assignee_user_id' "1" "assignee unchanged on self-start"

echo "[9] start fails on other-user-assigned task"
OTHER_ID="$(run_lf --output json task add --title "Other-assigned" --assignee-user-id "$USER2_ID" | jq -r '.id')"
run_lf task publish "$OTHER_ID" >/dev/null
START_ERR="$(run_lf task start "$OTHER_ID" 2>&1 || true)"
assert_contains "$START_ERR" "assigned to user" "start other-user-assigned fails with error"
assert_exit_code 1 run_lf task start "$OTHER_ID"

# === Section 4: next + assignee filtering ===

setup_test_env
export SENKO_USER="alice"
USER2_OUT="$(run_lf user create --username "bob")"
USER2_ID="$(echo "$USER2_OUT" | jq -r '.id')"

echo "[10] next selects self-assigned todo task"
MY_NEXT_ID="$(run_lf --output json task add --title "My next task" --assignee-user-id self | jq -r '.id')"
run_lf task publish "$MY_NEXT_ID" >/dev/null
NEXT_OUT="$(run_lf --output json task next)"
assert_eq "$MY_NEXT_ID" "$(echo "$NEXT_OUT" | jq -r '.id')" "next selects self-assigned task"
run_lf task complete "$MY_NEXT_ID" >/dev/null

echo "[11] next skips other-user-assigned task (no eligible)"
BOB_TASK="$(run_lf --output json task add --title "Bob only" --assignee-user-id "$USER2_ID" | jq -r '.id')"
run_lf task publish "$BOB_TASK" >/dev/null
NEXT_ERR="$(run_lf task next 2>&1 || true)"
assert_contains "$NEXT_ERR" "no eligible task" "next fails when only other-user tasks exist"
assert_exit_code 1 run_lf task next

echo "[12] next --include-unassigned selects unassigned task"
UNASSIGNED_ID="$(run_lf --output json task add --title "Unassigned next" | jq -r '.id')"
run_lf task publish "$UNASSIGNED_ID" >/dev/null
NEXT_OUT2="$(run_lf --output json task next --include-unassigned)"
assert_eq "$UNASSIGNED_ID" "$(echo "$NEXT_OUT2" | jq -r '.id')" "next --include-unassigned picks unassigned task"
run_lf task complete "$UNASSIGNED_ID" >/dev/null

echo "[13] next without --include-unassigned skips unassigned"
setup_test_env
export SENKO_USER="alice"
ONLY_UNASSIGNED_ID="$(run_lf --output json task add --title "Unassigned only" | jq -r '.id')"
run_lf task publish "$ONLY_UNASSIGNED_ID" >/dev/null
NEXT_ERR2="$(run_lf task next 2>&1 || true)"
assert_contains "$NEXT_ERR2" "no eligible task" "next without --include-unassigned skips unassigned tasks"

# === Section 5: list --ready + assignee filtering ===

setup_test_env
export SENKO_USER="alice"
USER2_OUT="$(run_lf user create --username "bob")"
USER2_ID="$(echo "$USER2_OUT" | jq -r '.id')"

echo "[14] list --ready shows self-assigned ready task, excludes other-user"
MINE_ID="$(run_lf --output json task add --title "My ready" --assignee-user-id self | jq -r '.id')"
BOB_READY_ID="$(run_lf --output json task add --title "Bob ready" --assignee-user-id "$USER2_ID" | jq -r '.id')"
run_lf task publish "$MINE_ID" >/dev/null
run_lf task publish "$BOB_READY_ID" >/dev/null
LIST_OUT="$(run_lf --output json task list --ready)"
MINE_COUNT="$(echo "$LIST_OUT" | jq --arg id "$MINE_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
BOB_COUNT="$(echo "$LIST_OUT" | jq --arg id "$BOB_READY_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$MINE_COUNT" "list --ready includes self-assigned task"
assert_eq "0" "$BOB_COUNT" "list --ready excludes other-user task"

echo "[15] list --ready --include-unassigned also shows unassigned"
NONE_ID="$(run_lf --output json task add --title "Unassigned ready" | jq -r '.id')"
run_lf task publish "$NONE_ID" >/dev/null
LIST_OUT2="$(run_lf --output json task list --ready --include-unassigned)"
MINE_COUNT2="$(echo "$LIST_OUT2" | jq --arg id "$MINE_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
NONE_COUNT="$(echo "$LIST_OUT2" | jq --arg id "$NONE_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
BOB_COUNT2="$(echo "$LIST_OUT2" | jq --arg id "$BOB_READY_ID" '[.[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$MINE_COUNT2" "list --ready --include-unassigned includes self-assigned"
assert_eq "1" "$NONE_COUNT" "list --ready --include-unassigned includes unassigned"
assert_eq "0" "$BOB_COUNT2" "list --ready --include-unassigned excludes other-user"

# === Section 6: from-json with assignee_user_id ===

setup_test_env
export SENKO_USER="alice"

echo "[16] add --from-json with assignee_user_id"
JSON_OUT="$(echo '{"title":"JSON assigned","assignee_user_id":1}' | run_lf --output json task add --from-json)"
assert_json_field "$JSON_OUT" '.assignee_user_id' "1" "from-json sets assignee_user_id"

echo "[17] add --from-json without assignee_user_id leaves null"
JSON_OUT2="$(echo '{"title":"JSON no assignee"}' | run_lf --output json task add --from-json)"
assert_json_field "$JSON_OUT2" '.assignee_user_id' "null" "from-json without assignee leaves null"

echo "[18] CLI --assignee-user-id overrides from-json input"
JSON_OUT3="$(echo '{"title":"JSON override","assignee_user_id":999}' | run_lf --output json task add --from-json --assignee-user-id self)"
assert_json_field "$JSON_OUT3" '.assignee_user_id' "1" "CLI --assignee-user-id overrides JSON"

# === Section 7: Backward compatibility (no SENKO_USER) ===

setup_test_env
# SENKO_USER is unset by setup_test_env

echo "[19] start works without SENKO_USER"
COMPAT_ID="$(run_lf --output json task add --title "No user start" | jq -r '.id')"
run_lf task publish "$COMPAT_ID" >/dev/null
COMPAT_START="$(run_lf --output json task start "$COMPAT_ID")"
assert_json_field "$COMPAT_START" '.status' "in_progress" "start without user succeeds"
assert_json_field "$COMPAT_START" '.assignee_user_id' "null" "start without user leaves assignee null"

echo "[20] next works without SENKO_USER (no filtering)"
run_lf task complete "$COMPAT_ID" >/dev/null
COMPAT_NEXT_ID="$(run_lf --output json task add --title "No user next" | jq -r '.id')"
run_lf task publish "$COMPAT_NEXT_ID" >/dev/null
COMPAT_NEXT="$(run_lf --output json task next)"
assert_json_field "$COMPAT_NEXT" '.status' "in_progress" "next without user works"

echo "[21] start preserves existing assignee when no user identity"
setup_test_env
PRESERVE_ID="$(run_lf --output json task add --title "Preserve assignee" --assignee-user-id 1 | jq -r '.id')"
run_lf task publish "$PRESERVE_ID" >/dev/null
PRESERVE_START="$(run_lf --output json task start "$PRESERVE_ID")"
assert_json_field "$PRESERVE_START" '.assignee_user_id' "1" "start without user preserves existing assignee"

test_summary
