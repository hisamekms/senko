#!/usr/bin/env bash
# e2e test: list --metadata filter (metadata search by key-value)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Metadata Search ---"

# Setup: define metadata fields for type resolution
run_lf project metadata-field add --name sprint --type string >/dev/null
run_lf project metadata-field add --name points --type number >/dev/null
run_lf project metadata-field add --name urgent --type boolean >/dev/null

# Create tasks with various metadata
A_ID="$(run_lf --output json task add --title "Alpha" --metadata '{"sprint":"v1","points":5,"urgent":true}' | jq -r '.id')"
B_ID="$(run_lf --output json task add --title "Beta" --metadata '{"sprint":"v2","points":3,"urgent":false}' | jq -r '.id')"
C_ID="$(run_lf --output json task add --title "Gamma" --metadata '{"sprint":"v1","points":8}' | jq -r '.id')"
D_ID="$(run_lf --output json task add --title "Delta" | jq -r '.id')"  # no metadata

# --- Case 1: Filter by string metadata ---
echo "[1] --metadata sprint=v1"
LIST="$(run_lf --output json task list --metadata sprint=v1)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "2" "$COUNT" "sprint=v1 returns 2 tasks (Alpha, Gamma)"

# --- Case 2: Filter by number metadata (auto-converted from string) ---
echo "[2] --metadata points=5"
LIST="$(run_lf --output json task list --metadata points=5)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "points=5 returns 1 task (Alpha)"
HAS_A="$(echo "$LIST" | jq --arg id "$A_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "1" "$HAS_A" "points=5 is Alpha"

# --- Case 3: Filter by boolean metadata ---
echo "[3] --metadata urgent=true"
LIST="$(run_lf --output json task list --metadata urgent=true)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "urgent=true returns 1 task (Alpha)"

echo "[3b] --metadata urgent=false"
LIST="$(run_lf --output json task list --metadata urgent=false)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "urgent=false returns 1 task (Beta)"

# --- Case 4: Combine metadata filters (AND) ---
echo "[4] --metadata sprint=v1 --metadata urgent=true"
LIST="$(run_lf --output json task list --metadata sprint=v1 --metadata urgent=true)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "sprint=v1 AND urgent=true returns 1 task"

# --- Case 5: No match ---
echo "[5] --metadata sprint=v3"
LIST="$(run_lf --output json task list --metadata sprint=v3)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "0" "$COUNT" "sprint=v3 returns 0 tasks"

# --- Case 6: Undefined field defaults to string comparison ---
echo "[6] --metadata custom=value (undefined field)"
E_ID="$(run_lf --output json task add --title "Epsilon" --metadata '{"custom":"value"}' | jq -r '.id')"
LIST="$(run_lf --output json task list --metadata custom=value)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "undefined field treated as string"

# --- Case 7: Combine metadata filter with status filter ---
echo "[7] --metadata sprint=v1 combined with --status draft"
LIST="$(run_lf --output json task list --metadata sprint=v1 --status draft)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "2" "$COUNT" "metadata + status filter works (Alpha, Gamma are draft)"

# Transition Alpha to todo to verify combined filter
run_lf task publish "$A_ID" >/dev/null
LIST="$(run_lf --output json task list --metadata sprint=v1 --status todo)"
COUNT="$(echo "$LIST" | jq '.items | length')"
assert_eq "1" "$COUNT" "sprint=v1 + status=todo returns only Alpha"

# --- Case 8: Task with no metadata excluded ---
echo "[8] tasks without metadata excluded"
LIST="$(run_lf --output json task list --metadata sprint=v1)"
HAS_D="$(echo "$LIST" | jq --arg id "$D_ID" '[.items[] | select(.id == ($id | tonumber))] | length')"
assert_eq "0" "$HAS_D" "Delta (no metadata) excluded"

test_summary
