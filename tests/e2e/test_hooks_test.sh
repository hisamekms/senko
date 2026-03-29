#!/usr/bin/env bash
# e2e test: hooks test subcommand

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: hooks test subcommand ---"

# Initialize DB
run_lf --output json list >/dev/null 2>&1

# Create a task to use for testing
TASK_ID="$(run_lf --output json add --title "Hook test task" --description "Test description" | jq -r '.id')"
run_lf ready "$TASK_ID" >/dev/null

# Configure a hook for task_ready
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[hooks.on_task_ready.cat-hook]
command = "cat"

[hooks.on_task_started.hook1]
command = "echo hook1"

[hooks.on_task_started.hook2]
command = "echo hook2"
EOF

# 1. dry-run: should output envelope JSON without executing hooks
echo "[1] dry-run outputs envelope JSON"
DRY_OUTPUT="$(run_lf hooks test task_ready "$TASK_ID" --dry-run 2>/dev/null)"
DRY_EVENT="$(echo "$DRY_OUTPUT" | jq -r '.event.event')"
DRY_TASK_ID="$(echo "$DRY_OUTPUT" | jq -r '.event.task.id')"
assert_eq "task_ready" "$DRY_EVENT" "dry-run event field"
assert_eq "$TASK_ID" "$DRY_TASK_ID" "dry-run task id"

# 1b. dry-run: envelope includes runtime and backend
echo "[1b] dry-run includes runtime and backend"
DRY_RUNTIME="$(echo "$DRY_OUTPUT" | jq -r '.runtime')"
DRY_BACKEND_TYPE="$(echo "$DRY_OUTPUT" | jq -r '.backend.type')"
assert_eq "cli" "$DRY_RUNTIME" "dry-run runtime field"
assert_eq "sqlite" "$DRY_BACKEND_TYPE" "dry-run backend type"
HAS_DB_PATH="$(echo "$DRY_OUTPUT" | jq '.backend | has("db_file_path")')"
assert_eq "true" "$HAS_DB_PATH" "dry-run backend has db_file_path"

# 2. dry-run without task_id: should use sample task
echo "[2] dry-run without task_id uses sample task"
SAMPLE_OUTPUT="$(run_lf hooks test task_added --dry-run 2>/dev/null)"
SAMPLE_TITLE="$(echo "$SAMPLE_OUTPUT" | jq -r '.event.task.title')"
assert_eq "Sample task" "$SAMPLE_TITLE" "sample task title"

# 3. hooks test with real execution: stdout should show envelope JSON (cat hook)
echo "[3] sync execution outputs to stdout"
EXEC_OUTPUT="$(run_lf hooks test task_ready "$TASK_ID" 2>/dev/null)"
EXEC_EVENT="$(echo "$EXEC_OUTPUT" | jq -r '.event.event')"
assert_eq "task_ready" "$EXEC_EVENT" "sync execution event"

# 4. exit code is displayed on stderr
echo "[4] exit code displayed on stderr"
STDERR_OUTPUT="$(run_lf hooks test task_ready "$TASK_ID" 2>&1 >/dev/null)"
assert_contains "$STDERR_OUTPUT" "exit code: 0" "exit code in stderr"

# 5. invalid event name
echo "[5] invalid event name errors"
INVALID_OUTPUT="$(run_lf hooks test invalid_event 2>&1 || true)"
assert_contains "$INVALID_OUTPUT" "unknown event" "invalid event error message"

# 6. no hooks configured for event
echo "[6] no hooks configured message"
NO_HOOK_STDERR="$(run_lf hooks test task_completed 2>&1 >/dev/null)"
assert_contains "$NO_HOOK_STDERR" "No hooks configured" "no hooks message"

# 7. multiple hooks executed
echo "[7] multiple hooks for same event"
MULTI_STDERR="$(run_lf hooks test task_started "$TASK_ID" 2>&1 >/dev/null)"
assert_contains "$MULTI_STDERR" "hook 1/2" "multi hook header 1"
assert_contains "$MULTI_STDERR" "hook 2/2" "multi hook header 2"

# 8. task state unchanged after test
echo "[8] task state unchanged"
STATUS_AFTER="$(run_lf get "$TASK_ID" | jq -r '.status')"
assert_eq "todo" "$STATUS_AFTER" "task status unchanged"

# 9. dry-run includes stats and ready_count inside event
echo "[9] dry-run includes stats"
STATS_OUTPUT="$(run_lf hooks test task_ready "$TASK_ID" --dry-run 2>/dev/null)"
HAS_STATS="$(echo "$STATS_OUTPUT" | jq '.event | has("stats")')"
HAS_READY="$(echo "$STATS_OUTPUT" | jq '.event | has("ready_count")')"
assert_eq "true" "$HAS_STATS" "dry-run has stats"
assert_eq "true" "$HAS_READY" "dry-run has ready_count"

# 10. no_eligible_task dry-run includes envelope
echo "[10] no_eligible_task dry-run includes envelope"
NO_TASK_OUTPUT="$(run_lf hooks test no_eligible_task --dry-run 2>/dev/null)"
NO_TASK_RUNTIME="$(echo "$NO_TASK_OUTPUT" | jq -r '.runtime')"
NO_TASK_EVENT="$(echo "$NO_TASK_OUTPUT" | jq -r '.event.event')"
assert_eq "cli" "$NO_TASK_RUNTIME" "no_eligible_task runtime"
assert_eq "no_eligible_task" "$NO_TASK_EVENT" "no_eligible_task event name"

test_summary
