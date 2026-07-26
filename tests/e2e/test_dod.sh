#!/usr/bin/env bash
# e2e test: dod check/uncheck + complete blocking

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: DoD Check/Uncheck ---"

# 1. dod check / uncheck
echo "[1] dod check and uncheck"

ADD_OUT="$(run_lf --output json task add --title "DoD Task" --definition-of-done "[manual] item1" --definition-of-done "[manual] item2")"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"

# All unchecked initially
CHECKED="$(echo "$ADD_OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false,false]' "$CHECKED" "initial: all unchecked"

# Check first item
OUT="$(run_lf --output json task dod check "$TASK_ID" 1)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[true,false]' "$CHECKED" "after check 1"

# Check second item
OUT="$(run_lf --output json task dod check "$TASK_ID" 2)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[true,true]' "$CHECKED" "after check 2"

# Uncheck first item
OUT="$(run_lf --output json task dod uncheck "$TASK_ID" 1)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false,true]' "$CHECKED" "after uncheck 1"

# 2. Index out of range
echo "[2] Index out of range"
assert_exit_code 1 run_lf --output json task dod check "$TASK_ID" 0
assert_exit_code 1 run_lf --output json task dod check "$TASK_ID" 3

# 3. Complete blocked by unchecked DoD
echo "[3] Complete blocked by unchecked DoD"

# Move task to in_progress (draft -> todo -> in_progress)
run_lf --output json task publish "$TASK_ID" >/dev/null
run_lf --output json task start "$TASK_ID" >/dev/null

# Attempt complete with unchecked items should fail
assert_exit_code 1 run_lf --output json task complete "$TASK_ID"

# 4. Complete succeeds after all DoD checked
echo "[4] Complete succeeds after all DoD checked"

# Check remaining unchecked item (item 1)
run_lf --output json task dod check "$TASK_ID" 1 >/dev/null

OUT="$(run_lf --output json task complete "$TASK_ID")"
assert_json_field "$OUT" '.status' "completed" "complete with all DoD checked"

# 5. Complete without DoD items succeeds
echo "[5] Complete without DoD items"

ADD_NODOD="$(run_lf --output json task add --title "No DoD Task")"
NODOD_ID="$(echo "$ADD_NODOD" | jq -r '.id')"
run_lf --output json task publish "$NODOD_ID" >/dev/null
run_lf --output json task start "$NODOD_ID" >/dev/null

OUT="$(run_lf --output json task complete "$NODOD_ID")"
assert_json_field "$OUT" '.status' "completed" "complete without DoD items"

# 6. Prefix format with verification method
echo "[6] Prefix format with verification method"

VT_OUT="$(run_lf --output json task add --title "Verification Task" \
  --definition-of-done "[execution] run suite :: mise test" \
  --definition-of-done "[static] types are sound")"
VT_ID="$(echo "$VT_OUT" | jq -r '.id')"

assert_json_field "$VT_OUT" '.definition_of_done[0].content' "run suite" "prefix: content stripped of tag and method"
assert_json_field "$VT_OUT" '.definition_of_done[0].verification_type' "execution" "prefix: verification_type execution"
assert_json_field "$VT_OUT" '.definition_of_done[0].verification_method' "mise test" "prefix: verification_method stored"
assert_json_field "$VT_OUT" '.definition_of_done[1].verification_type' "static" "prefix: verification_type static"
assert_json_field "$VT_OUT" '.definition_of_done[1].verification_method' "null" "prefix: no method is null"

# 7. Untagged and [unspecified] items are rejected
echo "[7] Untagged and [unspecified] items are rejected"
assert_exit_code 1 run_lf --output json task add --title "Plain DoD" --definition-of-done "plain item"
assert_exit_code 1 run_lf --output json task add --title "Unspecified DoD" --definition-of-done "[unspecified] item"

# 8. JSON-object form of --definition-of-done
echo "[8] JSON-object form of --definition-of-done"

JSON_OUT="$(run_lf --output json task add --title "JSON DoD Task" \
  --definition-of-done '{"content":"json item","verification_type":"manual","verification_method":"eyeball it"}')"
assert_json_field "$JSON_OUT" '.definition_of_done[0].content' "json item" "json-object: content"
assert_json_field "$JSON_OUT" '.definition_of_done[0].verification_type' "manual" "json-object: verification_type"
assert_json_field "$JSON_OUT" '.definition_of_done[0].verification_method' "eyeball it" "json-object: verification_method"

# 9. dod check --note stores verification_note; uncheck clears it
echo "[9] dod check --note and uncheck clearing"

OUT="$(run_lf --output json task dod check "$VT_ID" 1 --note "ran tests, all pass")"
assert_json_field "$OUT" '.definition_of_done[0].checked' "true" "note: item checked"
assert_json_field "$OUT" '.definition_of_done[0].verification_note' "ran tests, all pass" "note: verification_note stored"

OUT="$(run_lf --output json task dod uncheck "$VT_ID" 1)"
assert_json_field "$OUT" '.definition_of_done[0].checked' "false" "note: item unchecked"
assert_json_field "$OUT" '.definition_of_done[0].verification_note' "null" "note: uncheck clears verification_note"

test_summary
