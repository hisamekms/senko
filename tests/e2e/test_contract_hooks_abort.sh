#!/usr/bin/env bash
# e2e test: contract operations respect sync+pre+on_failure=abort.
#
# Mirrors test_hooks_abort.sh but exercises each contract write operation.
# Only sync+pre hooks with on_failure=abort should block the operation;
# async or post variants degrade to a warning.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: contract hooks sync+pre+abort ---"

mkdir -p "$TEST_PROJECT_ROOT/.senko"

# ---------------------------------------------------------------
# Case 1: sync + pre + on_failure=abort blocks `contract add`.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_add.hooks.blocker]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "abort"
TOML

BEFORE_COUNT="$(run_lf --output json contract list | jq '.items | length')"

echo "[1] contract add aborted by sync+pre+abort hook"
ADD_EXIT=0
ADD_OUTPUT="$(run_lf contract add --title "Abort Contract" 2>&1)" || ADD_EXIT=$?
if [[ "$ADD_EXIT" -ne 0 ]]; then
  echo "  PASS: contract add exited non-zero ($ADD_EXIT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: contract add exit code was 0 (expected non-zero)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$ADD_OUTPUT" "aborted by pre-hook" "contract add: aborted-by-pre-hook message"

AFTER_COUNT="$(run_lf --output json contract list | jq '.items | length')"
assert_eq "$BEFORE_COUNT" "$AFTER_COUNT" "contract list unchanged after abort"

# ---------------------------------------------------------------
# Case 2: sync + pre + abort blocks `contract dod check`.
# ---------------------------------------------------------------
# Reset config to allow contract creation, then set abort only on dod_check.
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_dod_check.hooks.blocker]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "abort"
TOML

CID="$(run_lf --output json contract add \
  --title "Abort DoD Contract" \
  --definition-of-done "[manual] item1" | jq -r '.id')"

BEFORE_DOD="$(run_lf --output json contract get "$CID" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false]' "$BEFORE_DOD" "initial dod state"

echo "[2] contract dod check aborted by sync+pre+abort hook"
DOD_EXIT=0
DOD_OUTPUT="$(run_lf contract dod check "$CID" 1 2>&1)" || DOD_EXIT=$?
if [[ "$DOD_EXIT" -ne 0 ]]; then
  echo "  PASS: contract dod check exited non-zero ($DOD_EXIT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: contract dod check exit code was 0 (expected non-zero)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$DOD_OUTPUT" "aborted by pre-hook" "contract dod check: aborted-by-pre-hook message"

AFTER_DOD="$(run_lf --output json contract get "$CID" | jq -c '[.definition_of_done[].checked]')"
assert_eq '[false]' "$AFTER_DOD" "dod state unchanged after abort"

# ---------------------------------------------------------------
# Case 3: sync + pre + abort blocks `contract note add`.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_note_add.hooks.blocker]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "abort"
TOML

BEFORE_NOTES="$(run_lf --output json contract note list "$CID" | jq '.items | length')"

echo "[3] contract note add aborted by sync+pre+abort hook"
NOTE_EXIT=0
NOTE_OUTPUT="$(run_lf contract note add "$CID" --content "blocked" 2>&1)" || NOTE_EXIT=$?
if [[ "$NOTE_EXIT" -ne 0 ]]; then
  echo "  PASS: contract note add exited non-zero ($NOTE_EXIT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: contract note add exit code was 0 (expected non-zero)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$NOTE_OUTPUT" "aborted by pre-hook" "contract note add: aborted-by-pre-hook message"

AFTER_NOTES="$(run_lf --output json contract note list "$CID" | jq '.items | length')"
assert_eq "$BEFORE_NOTES" "$AFTER_NOTES" "note list unchanged after abort"

# ---------------------------------------------------------------
# Case 4: async + pre + abort does NOT abort (load-time warning only).
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_add.hooks.async_abort_noop]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "async"
on_failure = "abort"
TOML

echo "[4] async+pre+abort does NOT abort contract add"
ASYNC_OUT="$(run_lf --output json contract add --title "Async noop" 2>/dev/null)"
ASYNC_ID="$(echo "$ASYNC_OUT" | jq -r '.id')"
sleep 0.5
GET_ASYNC="$(run_lf --output json contract get "$ASYNC_ID" | jq -r '.title')"
assert_eq "Async noop" "$GET_ASYNC" "async hook cannot abort — contract created"

# ---------------------------------------------------------------
# Case 5: sync + post + abort does NOT abort.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_add.hooks.post_abort_noop]
command = "cat >/dev/null; exit 1"
when = "post"
mode = "sync"
on_failure = "abort"
TOML

echo "[5] sync+post+abort does NOT abort contract add"
POST_OUT="$(run_lf --output json contract add --title "Post noop" 2>/dev/null)"
POST_ID="$(echo "$POST_OUT" | jq -r '.id')"
GET_POST="$(run_lf --output json contract get "$POST_ID" | jq -r '.title')"
assert_eq "Post noop" "$GET_POST" "post hook cannot abort — contract created"

# ---------------------------------------------------------------
# Case 6: sync + pre + warn proceeds even when hook fails.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.contract_add.hooks.warn_hook]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "warn"
TOML

echo "[6] sync+pre+warn continues despite hook failure"
WARN_OUT="$(run_lf --output json contract add --title "Warn only" 2>/dev/null)"
WARN_ID="$(echo "$WARN_OUT" | jq -r '.id')"
GET_WARN="$(run_lf --output json contract get "$WARN_ID" | jq -r '.title')"
assert_eq "Warn only" "$GET_WARN" "warn on_failure — contract created"

test_summary
