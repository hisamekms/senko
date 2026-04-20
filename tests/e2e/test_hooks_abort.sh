#!/usr/bin/env bash
# e2e test: sync + when="pre" + on_failure="abort" cancels a state transition.
#
# Only sync+pre hooks with on_failure=abort are allowed to abort the subsequent
# state transition (see infra/hook/mod.rs :: fire). Async hooks or post hooks
# with on_failure=abort degrade to a warning.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: hooks sync+pre+abort ---"

mkdir -p "$TEST_PROJECT_ROOT/.senko"

# ---------------------------------------------------------------
# Case 1: sync + pre + on_failure=abort should abort task publish.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.task_publish.hooks.blocker]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "abort"
TOML

TASK_ID="$(run_lf --output json task add --title "Abort test" | jq -r '.id')"
BEFORE_STATUS="$(run_lf task get "$TASK_ID" | jq -r '.status')"
assert_eq "draft" "$BEFORE_STATUS" "new task starts as draft"

echo "[1] task publish aborted by sync+pre+abort hook"
READY_EXIT=0
READY_OUTPUT="$(run_lf task publish "$TASK_ID" 2>&1)" || READY_EXIT=$?
if [[ "$READY_EXIT" -ne 0 ]]; then
  echo "  PASS: task publish exited non-zero ($READY_EXIT)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task publish exit code was 0 (expected non-zero)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$READY_OUTPUT" "aborted by pre-hook" "output mentions 'aborted by pre-hook'"

echo "[2] task status unchanged (still draft)"
AFTER_STATUS="$(run_lf task get "$TASK_ID" | jq -r '.status')"
assert_eq "draft" "$AFTER_STATUS" "status remains draft after abort"

# ---------------------------------------------------------------
# Case 2: async + pre + on_failure=abort must NOT abort (load-time warning).
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.task_publish.hooks.async_abort_noop]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "async"
on_failure = "abort"
TOML

TASK_ID2="$(run_lf --output json task add --title "Async abort noop" | jq -r '.id')"

echo "[3] async+pre+abort does NOT abort transition"
run_lf task publish "$TASK_ID2" >/dev/null 2>&1
# Give the async hook a moment before checking status
sleep 0.5
AFTER2="$(run_lf task get "$TASK_ID2" | jq -r '.status')"
assert_eq "todo" "$AFTER2" "async hook cannot abort — task moved to todo"

# ---------------------------------------------------------------
# Case 3: sync + post + on_failure=abort logs but does NOT abort.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.task_publish.hooks.post_abort_noop]
command = "cat >/dev/null; exit 1"
when = "post"
mode = "sync"
on_failure = "abort"
TOML

TASK_ID3="$(run_lf --output json task add --title "Post abort noop" | jq -r '.id')"

echo "[4] sync+post+abort does NOT abort transition"
run_lf task publish "$TASK_ID3" >/dev/null 2>&1
AFTER3="$(run_lf task get "$TASK_ID3" | jq -r '.status')"
assert_eq "todo" "$AFTER3" "post hook cannot abort — task moved to todo"

# ---------------------------------------------------------------
# Case 4: sync + pre + on_failure=warn succeeds even if hook fails.
# ---------------------------------------------------------------
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[cli.task_publish.hooks.warn_hook]
command = "cat >/dev/null; exit 1"
when = "pre"
mode = "sync"
on_failure = "warn"
TOML

TASK_ID4="$(run_lf --output json task add --title "Warn only" | jq -r '.id')"

echo "[5] sync+pre+warn continues despite hook failure"
run_lf task publish "$TASK_ID4" >/dev/null 2>&1
AFTER4="$(run_lf task get "$TASK_ID4" | jq -r '.status')"
assert_eq "todo" "$AFTER4" "warn on_failure — transition proceeds"

test_summary
