#!/usr/bin/env bash
# e2e test: contract DoD check/uncheck + is_completed semantics

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Contract DoD ---"

# 1. check / uncheck (1-based)
echo "[1] DoD check and uncheck"
ADD_OUT="$(run_lf --output json contract add \
  --title "DoD Contract" \
  --definition-of-done "[manual] item1" \
  --definition-of-done "[manual] item2")"
CID="$(echo "$ADD_OUT" | jq -r '.id')"

CHECKED="$(echo "$ADD_OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false,false]' "$CHECKED" "initial: all unchecked"
assert_json_field "$ADD_OUT" '.is_completed' "false" "initial: is_completed=false"

OUT="$(run_lf --output json contract dod check "$CID" 1)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[true,false]' "$CHECKED" "after check 1"
assert_json_field "$OUT" '.is_completed' "false" "one checked: is_completed=false"

OUT="$(run_lf --output json contract dod check "$CID" 2)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[true,true]' "$CHECKED" "after check 2"
assert_json_field "$OUT" '.is_completed' "true" "all checked: is_completed=true"

OUT="$(run_lf --output json contract dod uncheck "$CID" 1)"
CHECKED="$(echo "$OUT" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false,true]' "$CHECKED" "after uncheck 1"
assert_json_field "$OUT" '.is_completed' "false" "one unchecked again: is_completed=false"

# 2. Index out of range
echo "[2] Index out of range"
assert_exit_code 1 run_lf --output json contract dod check "$CID" 0
assert_exit_code 1 run_lf --output json contract dod check "$CID" 3
assert_exit_code 1 run_lf --output json contract dod uncheck "$CID" 0
assert_exit_code 1 run_lf --output json contract dod uncheck "$CID" 3

# 3. is_completed=false for contract with empty DoD
echo "[3] Empty DoD: is_completed=false"
EMPTY_ADD="$(run_lf --output json contract add --title "Empty DoD Contract")"
EMPTY_CID="$(echo "$EMPTY_ADD" | jq -r '.id')"
assert_json_field "$EMPTY_ADD" '.is_completed' "false" "empty DoD: is_completed=false on add"

EMPTY_GET="$(run_lf --output json contract get "$EMPTY_CID")"
assert_json_field "$EMPTY_GET" '.is_completed' "false" "empty DoD: is_completed=false on get"

# 4. is_completed transition (all check -> true, one uncheck -> false)
echo "[4] is_completed transition"
run_lf --output json contract dod check "$CID" 1 >/dev/null
OUT="$(run_lf --output json contract get "$CID")"
assert_json_field "$OUT" '.is_completed' "true" "after re-checking all: is_completed=true"

run_lf --output json contract dod uncheck "$CID" 2 >/dev/null
OUT="$(run_lf --output json contract get "$CID")"
assert_json_field "$OUT" '.is_completed' "false" "after uncheck one: is_completed=false"

# 5. Prefix format with verification method
echo "[5] Prefix format with verification method"
VT_ADD="$(run_lf --output json contract add \
  --title "Verification Contract" \
  --definition-of-done "[execution] run suite :: mise test" \
  --definition-of-done "[static] types are sound")"
VT_CID="$(echo "$VT_ADD" | jq -r '.id')"

assert_json_field "$VT_ADD" '.definition_of_done[0].content' "run suite" "prefix: content stripped of tag and method"
assert_json_field "$VT_ADD" '.definition_of_done[0].verification_type' "execution" "prefix: verification_type execution"
assert_json_field "$VT_ADD" '.definition_of_done[0].verification_method' "mise test" "prefix: verification_method stored"
assert_json_field "$VT_ADD" '.definition_of_done[1].verification_type' "static" "prefix: verification_type static"
assert_json_field "$VT_ADD" '.definition_of_done[1].verification_method' "null" "prefix: no method is null"

# 6. Untagged and [unspecified] items are rejected
echo "[6] Untagged and [unspecified] items are rejected"
assert_exit_code 1 run_lf --output json contract add --title "Plain DoD" --definition-of-done "plain item"
assert_exit_code 1 run_lf --output json contract add --title "Unspecified DoD" --definition-of-done "[unspecified] item"

# 7. JSON-object form of --definition-of-done
echo "[7] JSON-object form of --definition-of-done"
JSON_ADD="$(run_lf --output json contract add --title "JSON DoD Contract" \
  --definition-of-done '{"content":"json item","verification_type":"manual","verification_method":"eyeball it"}')"
assert_json_field "$JSON_ADD" '.definition_of_done[0].content' "json item" "json-object: content"
assert_json_field "$JSON_ADD" '.definition_of_done[0].verification_type' "manual" "json-object: verification_type"
assert_json_field "$JSON_ADD" '.definition_of_done[0].verification_method' "eyeball it" "json-object: verification_method"

# 8. dod check --note stores verification_note; uncheck clears it
echo "[8] dod check --note and uncheck clearing"
OUT="$(run_lf --output json contract dod check "$VT_CID" 1 --note "ran tests, all pass")"
assert_json_field "$OUT" '.definition_of_done[0].checked' "true" "note: item checked"
assert_json_field "$OUT" '.definition_of_done[0].verification_note' "ran tests, all pass" "note: verification_note stored"

OUT="$(run_lf --output json contract dod uncheck "$VT_CID" 1)"
assert_json_field "$OUT" '.definition_of_done[0].checked' "false" "note: item unchecked"
assert_json_field "$OUT" '.definition_of_done[0].verification_note' "null" "note: uncheck clears verification_note"

test_summary
