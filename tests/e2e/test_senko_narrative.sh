#!/usr/bin/env bash
# e2e test: senko-narrative.sh + senko-gc.sh helpers
#
# Covers DoD items 1-2 of senko task 375:
#   - All senko-narrative.sh subcommands behave as specified
#   - senko-gc.sh and lazy-cleanup remove cutoff-stale state dirs

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
NARRATIVE="$PROJECT_ROOT/.claude/skills/senko/scripts/senko-narrative.sh"
GC="$PROJECT_ROOT/.claude/skills/senko/scripts/senko-gc.sh"
STATE_DIR="$XDG_STATE_HOME/senko"

# Wrapper that injects the e2e --project-root and DB args into every senko call
# so build-packet (which spawns senko under the hood) can reach the test DB.
SENKO_WRAPPER="$TEST_DIR/senko-wrapper.sh"
{
  printf '#!/usr/bin/env bash\n'
  printf 'exec "%s" --project-root "%s"' "$SENKO" "$TEST_PROJECT_ROOT"
  for arg in "${SENKO_DB_ARGS[@]}"; do
    printf ' "%s"' "$arg"
  done
  printf ' "$@"\n'
} > "$SENKO_WRAPPER"
chmod +x "$SENKO_WRAPPER"
export SENKO_BIN="$SENKO_WRAPPER"

echo "--- Test: senko-narrative.sh ---"

# 1. init: produces valid ID, creates state dir, narrative.md, metadata.json
echo "[1] init creates ID and state dir"
NID="$(echo '{"intent":"add narrative helper"}' | bash "$NARRATIVE" init)"
if [[ "$NID" =~ ^[a-zA-Z0-9]{8}$ ]]; then
  echo "  PASS: ID matches ^[a-zA-Z0-9]{8}$ ($NID)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: ID '$NID' does not match expected pattern"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
[[ -d "$STATE_DIR/$NID" ]] && { echo "  PASS: state dir created"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: state dir missing"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
[[ -f "$STATE_DIR/$NID/metadata.json" ]] && { echo "  PASS: metadata.json created"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: metadata.json missing"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
[[ -f "$STATE_DIR/$NID/narrative.md" ]] && { echo "  PASS: narrative.md created"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: narrative.md missing"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

# Verify narrative.md has all three section headings
NARR_CONTENT="$(cat "$STATE_DIR/$NID/narrative.md")"
assert_contains "$NARR_CONTENT" "# Original user intent" "narrative has 'Original user intent' heading"
assert_contains "$NARR_CONTENT" "# Decisions" "narrative has 'Decisions' heading"
assert_contains "$NARR_CONTENT" "# Known constraints" "narrative has 'Known constraints' heading"
assert_contains "$NARR_CONTENT" "add narrative helper" "narrative includes original intent text"

# 2. append-decision (q+a)
echo "[2] append-decision q+a"
echo '{"q":"Use awk?","a":"Yes"}' | bash "$NARRATIVE" append-decision "$NID"
NARR_CONTENT="$(cat "$STATE_DIR/$NID/narrative.md")"
assert_contains "$NARR_CONTENT" "- Use awk?: Yes" "decision line appended"

# 3. append-decision (note)
echo "[3] append-decision note"
echo '{"note":"Picked mktemp for atomicity"}' | bash "$NARRATIVE" append-decision "$NID"
NARR_CONTENT="$(cat "$STATE_DIR/$NID/narrative.md")"
assert_contains "$NARR_CONTENT" "- (note) Picked mktemp for atomicity" "note line appended"

# 4. append-decision idempotent
echo "[4] append-decision is idempotent on duplicate input"
echo '{"q":"Use awk?","a":"Yes"}' | bash "$NARRATIVE" append-decision "$NID"
DUP_COUNT="$(grep -Fxc -- "- Use awk?: Yes" "$STATE_DIR/$NID/narrative.md" || true)"
assert_eq "1" "$DUP_COUNT" "duplicate decision line is not re-added"

# 5. append-constraint
echo "[5] append-constraint"
echo '{"text":"Must work without senko CLI for unit tests"}' | bash "$NARRATIVE" append-constraint "$NID"
NARR_CONTENT="$(cat "$STATE_DIR/$NID/narrative.md")"
assert_contains "$NARR_CONTENT" "- Must work without senko CLI for unit tests" "constraint appended"

# 6. path subcommand
echo "[6] path subcommand prints absolute path to narrative.md"
PATH_OUT="$(bash "$NARRATIVE" path "$NID")"
assert_eq "$STATE_DIR/$NID/narrative.md" "$PATH_OUT" "path returns narrative.md absolute path"

# 7. packet-path subcommand
echo "[7] packet-path returns absolute packet.md path even before build"
PPATH_OUT="$(bash "$NARRATIVE" packet-path "$NID")"
assert_eq "$STATE_DIR/$NID/packet.md" "$PPATH_OUT" "packet-path returns packet.md absolute path"

# 8. Invalid ID rejection
echo "[8] Invalid IDs are rejected with non-zero exit"
assert_exit_code 2 bash "$NARRATIVE" path "../foo"
assert_exit_code 2 bash "$NARRATIVE" path "xxx"
assert_exit_code 2 bash "$NARRATIVE" path "id_with_underscore"

# 9. Nonexistent (but well-formed) ID rejected
echo "[9] Nonexistent valid-format ID is rejected with non-zero exit"
assert_exit_code 3 bash "$NARRATIVE" path "abcdEFGH"

# 10. build-packet --mode single (real senko, single task)
echo "[10] build-packet --mode single produces packet without Contract section"
TASK_ID="$(run_lf task add --title "Smoke task A" --priority p2 | jq -r '.id')"
PACKET_PATH="$(bash "$NARRATIVE" build-packet "$NID" --mode single --tasks "$TASK_ID")"
assert_eq "$STATE_DIR/$NID/packet.md" "$PACKET_PATH" "build-packet prints packet.md path"
[[ -f "$PACKET_PATH" ]] && { echo "  PASS: packet.md exists"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: packet.md missing"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
PACKET_CONTENT="$(cat "$PACKET_PATH")"
assert_contains "$PACKET_CONTENT" "# Mode: single" "packet declares single mode"
assert_contains "$PACKET_CONTENT" "## Task $TASK_ID" "packet contains task heading"
assert_contains "$PACKET_CONTENT" "Smoke task A" "packet includes task body"
assert_not_contains "$PACKET_CONTENT" "# Contract" "single-mode packet has no Contract section"

# Metadata reflects single mode
META="$(cat "$STATE_DIR/$NID/metadata.json")"
assert_json_field "$META" '.mode' "single" "metadata.mode == single"
assert_json_field "$META" '.contract_id' "null" "metadata.contract_id == null"

# 11. build-packet --mode split (real senko, contract + tasks)
echo "[11] build-packet --mode split includes Contract section"
CONTRACT_ID="$(run_lf contract add --title "Smoke contract" --description "for e2e" --definition-of-done "DoD A" | jq -r '.id')"
run_lf contract note add "$CONTRACT_ID" --content "first note" >/dev/null
TASK_ID2="$(run_lf task add --title "Smoke task B" --priority p2 | jq -r '.id')"
PACKET_PATH2="$(bash "$NARRATIVE" build-packet "$NID" --mode split --contract "$CONTRACT_ID" --tasks "$TASK_ID" "$TASK_ID2")"
assert_eq "$STATE_DIR/$NID/packet.md" "$PACKET_PATH2" "build-packet split prints packet.md path"
PACKET_CONTENT="$(cat "$PACKET_PATH2")"
assert_contains "$PACKET_CONTENT" "# Mode: split" "packet declares split mode"
assert_contains "$PACKET_CONTENT" "# Contract" "split-mode packet has Contract section"
assert_contains "$PACKET_CONTENT" "## Contract $CONTRACT_ID" "packet contains contract heading"
assert_contains "$PACKET_CONTENT" "Smoke contract" "packet includes contract body"
assert_contains "$PACKET_CONTENT" "first note" "packet includes contract notes"
assert_contains "$PACKET_CONTENT" "## Task $TASK_ID" "packet contains first task"
assert_contains "$PACKET_CONTENT" "## Task $TASK_ID2" "packet contains second task"

META="$(cat "$STATE_DIR/$NID/metadata.json")"
assert_json_field "$META" '.mode' "split" "metadata.mode == split"
assert_json_field "$META" '.contract_id' "$CONTRACT_ID" "metadata.contract_id reflects --contract"

# 12. build-packet overwrites on re-run with different --tasks
echo "[12] build-packet re-run with different --tasks overwrites packet.md"
bash "$NARRATIVE" build-packet "$NID" --mode single --tasks "$TASK_ID2" >/dev/null
PACKET_CONTENT="$(cat "$PACKET_PATH")"
assert_not_contains "$PACKET_CONTENT" "# Contract" "Contract section gone after switching to single"
assert_not_contains "$PACKET_CONTENT" "## Task $TASK_ID" "first task gone after switching task list"
assert_contains "$PACKET_CONTENT" "## Task $TASK_ID2" "remaining task still present"

# 13. build-packet arg validation
echo "[13] build-packet arg validation"
assert_exit_code 2 bash "$NARRATIVE" build-packet "$NID" --mode bad --tasks "$TASK_ID"
assert_exit_code 2 bash "$NARRATIVE" build-packet "$NID" --mode split --tasks "$TASK_ID"  # no --contract
assert_exit_code 2 bash "$NARRATIVE" build-packet "$NID" --mode single  # no --tasks

echo "--- Test: senko-gc.sh ---"

# 14. senko-gc.sh removes cutoff-stale entries
echo "[14] senko-gc.sh removes state dirs older than cutoff"
NOLD="$(echo '{"intent":"will be swept"}' | bash "$NARRATIVE" init)"
NFRESH="$(echo '{"intent":"will be kept"}' | bash "$NARRATIVE" init)"
touch -d '14 days ago' "$STATE_DIR/$NOLD/metadata.json"
SENKO_GC_CUTOFF_DAYS=7 bash "$GC"
[[ ! -d "$STATE_DIR/$NOLD" ]] && { echo "  PASS: old state dir removed"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: old state dir still exists"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
[[ -d "$STATE_DIR/$NFRESH" ]] && { echo "  PASS: fresh state dir kept"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: fresh state dir was removed"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

echo "--- Test: lazy GC ---"

# 15. Lazy GC fires when .last-gc is older than 1h
echo "[15] Lazy GC sweeps when .last-gc is >1h old"
NSTALE="$(echo '{"intent":"stale"}' | bash "$NARRATIVE" init)"
touch -d '14 days ago' "$STATE_DIR/$NSTALE/metadata.json"
touch -d '2 hours ago' "$STATE_DIR/.last-gc"
# Trigger lazy GC via another init
SENKO_GC_CUTOFF_DAYS=7 bash "$NARRATIVE" init <<< '{"intent":"trigger lazy gc"}' >/dev/null
[[ ! -d "$STATE_DIR/$NSTALE" ]] && { echo "  PASS: lazy GC swept stale entry"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: lazy GC did not sweep stale entry"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

# 16. Lazy GC throttled when .last-gc is fresh
echo "[16] Lazy GC throttles when .last-gc is fresh (<1h)"
NSTALE2="$(echo '{"intent":"stale2"}' | bash "$NARRATIVE" init)"
touch -d '14 days ago' "$STATE_DIR/$NSTALE2/metadata.json"
touch "$STATE_DIR/.last-gc"  # fresh marker (now)
SENKO_GC_CUTOFF_DAYS=7 bash "$NARRATIVE" init <<< '{"intent":"second"}' >/dev/null
[[ -d "$STATE_DIR/$NSTALE2" ]] && { echo "  PASS: lazy GC throttled (entry preserved)"; PASS_COUNT=$((PASS_COUNT + 1)); } \
  || { echo "  FAIL: lazy GC fired when it shouldn't have"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

test_summary
