#!/usr/bin/env bash
# e2e test: doctor subcommand (hook configuration diagnostics)
#
# Under the new hooks schema the diagnostic `event` label is
# `<runtime>.<action>` (e.g. `cli.task_add`, `server.remote.task_publish`,
# `workflow.branch_merge`). Env-var requirements are declared via the
# structured `env_vars` list instead of the legacy `requires_env` array.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Doctor Subcommand ---"

# Initialize DB
run_lf --output json task list >/dev/null 2>&1

# 1. No hooks configured
echo "[1] No hooks configured"

OUT="$(run_lf --output text doctor)"
assert_contains "$OUT" "No hooks configured" "text output: no hooks"
assert_contains "$OUT" "all checks passed" "text output: all checks passed"

JSON="$(run_lf --output json doctor)"
HOOKS_LEN="$(echo "$JSON" | jq '.hooks | length')"
assert_eq "0" "$HOOKS_LEN" "json: empty hooks array"
assert_json_field "$JSON" '.has_errors' "false" "json: has_errors is false"

# 2. Bare command (no script path → no file checks)
echo "[2] Bare command — no file checks"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[cli.task_add.hooks.echo-hook]
command = "echo hello"
EOF

JSON="$(run_lf --output json doctor)"
HOOKS_LEN="$(echo "$JSON" | jq '.hooks | length')"
assert_eq "1" "$HOOKS_LEN" "json: one hook entry"
CHECKS_LEN="$(echo "$JSON" | jq '.hooks[0].checks | length')"
assert_eq "0" "$CHECKS_LEN" "json: bare command has no checks"
assert_json_field "$JSON" '.has_errors' "false" "json: has_errors false for bare cmd"
assert_json_field "$JSON" '.hooks[0].event' "cli.task_add" "json: event label is cli.task_add"

OUT="$(run_lf --output text doctor)"
assert_contains "$OUT" "all checks passed" "text: bare command passes"

# 3. Script exists and is executable
echo "[3] Script exists and is executable"

HOOK_SCRIPT="$TEST_DIR/good-hook.sh"
cat > "$HOOK_SCRIPT" <<'SCRIPT'
#!/bin/sh
echo ok
SCRIPT
chmod +x "$HOOK_SCRIPT"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_add.hooks.script-hook]
command = "$HOOK_SCRIPT"
EOF

JSON="$(run_lf --output json doctor)"
assert_json_field "$JSON" '.has_errors' "false" "json: executable script has no errors"
CHECKS_LEN="$(echo "$JSON" | jq '.hooks[0].checks | length')"
assert_eq "2" "$CHECKS_LEN" "json: script_exists + script_executable checks"
assert_json_field "$JSON" '.hooks[0].checks[0].check' "script_exists" "json: first check is script_exists"
assert_json_field "$JSON" '.hooks[0].checks[0].status' "ok" "json: script_exists ok"
assert_json_field "$JSON" '.hooks[0].checks[1].check' "script_executable" "json: second check is script_executable"
assert_json_field "$JSON" '.hooks[0].checks[1].status' "ok" "json: script_executable ok"

# 4. Script does not exist → exit 1
echo "[4] Script does not exist"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[cli.task_add.hooks.missing-hook]
command = "/nonexistent/path/hook.sh"
EOF

assert_exit_code 1 run_lf --output json doctor

JSON="$(run_lf --output json doctor 2>/dev/null || true)"
assert_json_field "$JSON" '.has_errors' "true" "json: has_errors true for missing script"
assert_json_field "$JSON" '.hooks[0].checks[0].check' "script_exists" "json: check is script_exists"
assert_json_field "$JSON" '.hooks[0].checks[0].status' "error" "json: script_exists error"

OUT="$(run_lf --output text doctor 2>/dev/null || true)"
assert_contains "$OUT" "issue(s) found" "text: issues found for missing script"

# 5. Script not executable → exit 1
echo "[5] Script not executable"

NOEXEC_SCRIPT="$TEST_DIR/noexec-hook.sh"
cat > "$NOEXEC_SCRIPT" <<'SCRIPT'
#!/bin/sh
echo ok
SCRIPT
chmod 644 "$NOEXEC_SCRIPT"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_add.hooks.noexec-hook]
command = "$NOEXEC_SCRIPT"
EOF

assert_exit_code 1 run_lf --output json doctor

JSON="$(run_lf --output json doctor 2>/dev/null || true)"
assert_json_field "$JSON" '.has_errors' "true" "json: has_errors true for non-executable"
assert_json_field "$JSON" '.hooks[0].checks[1].check' "script_executable" "json: check is script_executable"
assert_json_field "$JSON" '.hooks[0].checks[1].status' "error" "json: script_executable error"
assert_json_field "$JSON" '.hooks[0].checks[1].message' "not executable" "json: message is 'not executable'"

# 6. Missing required env var → exit 1
echo "[6] Missing required environment variable"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[cli.task_add.hooks.env-hook]
command = "echo test"
[[cli.task_add.hooks.env-hook.env_vars]]
name = "SENKO_E2E_DOCTOR_NONEXISTENT_VAR_12345"
required = true
EOF

assert_exit_code 1 run_lf --output json doctor

JSON="$(run_lf --output json doctor 2>/dev/null || true)"
assert_json_field "$JSON" '.has_errors' "true" "json: has_errors true for missing env"
assert_json_field "$JSON" '.hooks[0].checks[0].check' "env_var" "json: check is env_var"
assert_json_field "$JSON" '.hooks[0].checks[0].status' "error" "json: env_var error"
assert_contains "$(echo "$JSON" | jq -r '.hooks[0].checks[0].message')" "is not set" "json: message contains 'is not set'"

# 7. Environment variable present → ok
echo "[7] Environment variable present"

export SENKO_E2E_DOCTOR_TEST_VAR="1"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[cli.task_add.hooks.env-hook]
command = "echo test"
[[cli.task_add.hooks.env-hook.env_vars]]
name = "SENKO_E2E_DOCTOR_TEST_VAR"
required = true
EOF

JSON="$(run_lf --output json doctor)"
assert_json_field "$JSON" '.has_errors' "false" "json: has_errors false with env set"
assert_json_field "$JSON" '.hooks[0].checks[0].check' "env_var" "json: check is env_var"
assert_json_field "$JSON" '.hooks[0].checks[0].status' "ok" "json: env_var ok"

unset SENKO_E2E_DOCTOR_TEST_VAR

# 8. JSON output structure
echo "[8] JSON output structure"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_publish.hooks.my-hook]
command = "$HOOK_SCRIPT"
EOF

JSON="$(run_lf --output json doctor)"
# Verify top-level structure
assert_eq "true" "$(echo "$JSON" | jq 'has("hooks") and has("has_errors")')" "json: top-level has hooks and has_errors"
# Verify hook entry structure
assert_json_field "$JSON" '.hooks[0].event' "cli.task_publish" "json: hook event field"
assert_json_field "$JSON" '.hooks[0].name' "my-hook" "json: hook name field"
assert_eq "$HOOK_SCRIPT" "$(echo "$JSON" | jq -r '.hooks[0].command')" "json: hook command field"

# 9. Text output format
echo "[9] Text output format"

OUT="$(run_lf --output text doctor)"
assert_contains "$OUT" "Hook diagnostics" "text: header present"
assert_contains "$OUT" "[cli.task_publish] my-hook" "text: event/name header format"
assert_contains "$OUT" "command: $HOOK_SCRIPT" "text: command line"
assert_contains "$OUT" "all checks passed" "text: result line"

# 10. Disabled hooks are skipped
echo "[10] Disabled hooks are skipped"

cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_add.hooks.disabled-hook]
command = "/nonexistent/path/hook.sh"
enabled = false

[cli.task_add.hooks.enabled-hook]
command = "echo hello"
EOF

JSON="$(run_lf --output json doctor)"
HOOKS_LEN="$(echo "$JSON" | jq '.hooks | length')"
assert_eq "1" "$HOOKS_LEN" "json: only enabled hook appears"
assert_json_field "$JSON" '.hooks[0].name' "enabled-hook" "json: enabled hook is listed"
assert_json_field "$JSON" '.has_errors' "false" "json: disabled error hook doesn't affect result"

test_summary
