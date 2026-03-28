#!/usr/bin/env bash
# e2e tests for config subcommand
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Config ---"

echo "[1] config shows defaults when no config file exists"
JSON_OUT="$(run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.completion_mode' "merge_then_complete" "default completion_mode"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "true" "default auto_merge"

echo "[2] config text output shows defaults"
TEXT_OUT="$(run_lf --output text config)"
assert_contains "$TEXT_OUT" "merge_then_complete" "text shows completion_mode"
assert_contains "$TEXT_OUT" "auto_merge: true" "text shows auto_merge"

echo "[3] config --init creates config file"
INIT_OUT="$(run_lf config --init)"
assert_json_field "$INIT_OUT" '.action' "created" "init action is created"
if [[ -f "$TEST_PROJECT_ROOT/.senko/config.toml" ]]; then
  echo "  PASS: config.toml file created"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: config.toml file not created"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[4] config --init fails when file already exists"
INIT2_OUT="$(run_lf config --init 2>&1 || true)"
assert_contains "$INIT2_OUT" "already exists" "init fails with existing file"

echo "[5] config reads custom values from config.toml"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[workflow]
completion_mode = "pr_then_complete"
auto_merge = false

[hooks.on_task_added.my-hook]
command = "echo added"
EOF
CUSTOM_OUT="$(run_lf config)"
assert_json_field "$CUSTOM_OUT" '.workflow.completion_mode' "pr_then_complete" "custom completion_mode"
assert_json_field "$CUSTOM_OUT" '.workflow.auto_merge' "false" "custom auto_merge"

echo "[6] config text output shows custom values"
TEXT_CUSTOM="$(run_lf --output text config)"
assert_contains "$TEXT_CUSTOM" "pr_then_complete" "text shows custom completion_mode"
assert_contains "$TEXT_CUSTOM" "auto_merge: false" "text shows custom auto_merge"

echo "[7] config --init text output"
rm "$TEST_PROJECT_ROOT/.senko/config.toml"
INIT_TEXT="$(run_lf --output text config --init)"
assert_contains "$INIT_TEXT" "Created" "text init shows Created"

test_summary
