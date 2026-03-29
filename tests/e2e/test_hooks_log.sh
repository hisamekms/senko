#!/usr/bin/env bash
# e2e test: Hooks log subcommand (log, --path, --clear, -n)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Hooks Log ---"

# 1. --path returns a file path
echo "[1] hooks log --path"
LOG_PATH="$(run_lf hooks log --path)"
assert_contains "$LOG_PATH" "hooks.log" "path contains hooks.log"

# 2. --clear when no log file may or may not exist
echo "[2] hooks log --clear (idempotent)"
CLEAR_OUTPUT="$(run_lf hooks log --clear 2>&1)"
# Output is either "Cleared <path>" or "No log file to clear"
assert_contains "$CLEAR_OUTPUT" "log" "clear output mentions log"

# 3. Setup: configure a hook and trigger it to generate log entries
echo "[3] Generate log entries via hooks"
mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'TOML'
[hooks.on_task_added.test_hook]
command = "true"
enabled = true
TOML

# Clear the log first
run_lf hooks log --clear >/dev/null 2>&1

# Create tasks to trigger hook events
run_lf add --title "Hook log test 1" >/dev/null 2>&1
run_lf add --title "Hook log test 2" >/dev/null 2>&1
run_lf add --title "Hook log test 3" >/dev/null 2>&1

# Wait for async hooks to complete
sleep 1

# 4. hooks log shows entries
echo "[4] hooks log shows entries"
LOG_OUTPUT="$(run_lf hooks log 2>&1)"
assert_contains "$LOG_OUTPUT" "task_added" "log contains task_added events"

# 5. hooks log -n limits output
echo "[5] hooks log -n 1"
LOG_N1="$(run_lf hooks log -n 1 2>&1)"
LINE_COUNT="$(echo "$LOG_N1" | grep -c '{' || true)"
assert_eq "1" "$LINE_COUNT" "log -n 1 shows 1 entry"

# 6. hooks log --clear removes entries
echo "[6] hooks log --clear removes entries"
run_lf hooks log --clear >/dev/null 2>&1
LOG_AFTER_CLEAR="$(run_lf hooks log 2>&1)"
# After clearing, the log should be empty (no JSONL entries)
ENTRY_COUNT="$(echo "$LOG_AFTER_CLEAR" | grep -c '^{' || true)"
assert_eq "0" "$ENTRY_COUNT" "log is empty after clear"

test_summary
