#!/usr/bin/env bash
# e2e test: Task ↔ Contract linkage via --contract / --clear-contract / from-json

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Task-Contract Link ---"

# Setup: create a contract to link against
CID="$(run_lf --output json contract add --title "Link Target" --definition-of-done "[manual] x" | jq -r '.id')"

# 1. edit --contract links an existing task
echo "[1] edit --contract sets contract_id"
TID="$(run_lf --output json task add --title "Task A" | jq -r '.id')"
ADD_JSON="$(run_lf --output json task get "$TID")"
assert_json_field "$ADD_JSON" '.contract_id' "null" "initial: contract_id is null"

OUT="$(run_lf --output json task edit "$TID" --contract "$CID")"
assert_json_field "$OUT" '.contract_id' "$CID" "edit --contract: contract_id set"

GOT="$(run_lf --output json task get "$TID")"
assert_json_field "$GOT" '.contract_id' "$CID" "get after link: contract_id persisted"

# 2. edit --clear-contract removes the link
echo "[2] edit --clear-contract removes link"
OUT="$(run_lf --output json task edit "$TID" --clear-contract)"
assert_json_field "$OUT" '.contract_id' "null" "edit --clear-contract: contract_id null"

# 3. edit --contract with nonexistent contract fails
echo "[3] edit --contract with nonexistent id fails"
assert_exit_code 1 run_lf --output json task edit "$TID" --contract 99999

# 4. add --from-json with contract_id links at creation
echo "[4] add --from-json with contract_id sets link at creation"
NEW_TID="$(echo "{\"title\":\"Task B\",\"contract_id\":$CID}" \
  | run_lf --output json task add --from-json | jq -r '.id')"
GOT_B="$(run_lf --output json task get "$NEW_TID")"
assert_json_field "$GOT_B" '.contract_id' "$CID" "from-json: contract_id persisted"

# 5. add --from-json with nonexistent contract_id fails
echo "[5] add --from-json with nonexistent contract_id fails"
FAKE_JSON='{"title":"Bad Link","contract_id":99999}'
if echo "$FAKE_JSON" | run_lf --output json task add --from-json >/dev/null 2>&1; then
  echo "  FAIL: add --from-json with bad contract_id should have failed"
  FAIL_COUNT=$((FAIL_COUNT + 1))
else
  echo "  PASS: add --from-json with bad contract_id exits nonzero"
  PASS_COUNT=$((PASS_COUNT + 1))
fi

# 6. contract delete cascades: linked task.contract_id becomes null
echo "[6] contract delete nulls out linked task.contract_id"
CID2="$(run_lf --output json contract add --title "Cascade Target" | jq -r '.id')"
CASCADE_TID="$(run_lf --output json task add --title "Task C" | jq -r '.id')"
run_lf --output json task edit "$CASCADE_TID" --contract "$CID2" >/dev/null

BEFORE="$(run_lf --output json task get "$CASCADE_TID")"
assert_json_field "$BEFORE" '.contract_id' "$CID2" "before delete: contract_id=$CID2"

run_lf --output json contract delete "$CID2" >/dev/null

AFTER="$(run_lf --output json task get "$CASCADE_TID")"
assert_json_field "$AFTER" '.contract_id' "null" "after contract delete: task.contract_id nulled"

test_summary
