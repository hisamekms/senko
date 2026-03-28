#!/usr/bin/env bash
# e2e test: JSON error output format

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: JSON Error Output ---"

# ===== [1] JSON mode: error is valid JSON on stdout =====

echo "[1] JSON mode: non-existent task returns JSON error on stdout"
JSON_OUT="$(run_lf --output json get 99999 2>/dev/null || true)"
assert_contains "$JSON_OUT" '"error"' "stdout contains error key"

# Validate it's valid JSON
echo "$JSON_OUT" | jq -e '.error' >/dev/null 2>&1
assert_eq "0" "$?" "output is valid JSON with error field"

# ===== [2] JSON mode: exit code is 1 =====

echo "[2] JSON mode: exit code is 1 on error"
assert_exit_code 1 run_lf --output json get 99999

# ===== [3] Text mode: error goes to stderr =====

echo "[3] Text mode: error goes to stderr"
TEXT_STDERR="$(run_lf --output text get 99999 2>&1 1>/dev/null || true)"
assert_contains "$TEXT_STDERR" "Error:" "stderr contains Error: prefix"

# ===== [4] JSON mode: invalid state transition =====

echo "[4] JSON mode: invalid state transition"
ADD_OUT="$(run_lf --output json add --title "State Test")"
TASK_ID="$(echo "$ADD_OUT" | jq -r '.id')"
run_lf complete "$TASK_ID" >/dev/null 2>&1 || true
# Try to complete again - should fail
JSON_ERR="$(run_lf --output json complete "$TASK_ID" 2>/dev/null || true)"
assert_contains "$JSON_ERR" '"error"' "invalid transition returns JSON error"

# ===== [5] JSON mode: no stderr leak =====

echo "[5] JSON mode: no 'Error:' on stderr (only warnings allowed)"
STDERR_OUT="$(run_lf --output json get 99999 2>&1 1>/dev/null || true)"
# Filter out known warnings (e.g. .senko gitignore warning)
STDERR_ERRORS="$(echo "$STDERR_OUT" | grep -v "^warning:" || true)"
assert_eq "" "$STDERR_ERRORS" "no Error: on stderr in JSON mode"

# ===== [6] Text mode: exit code is 1 =====

echo "[6] Text mode: exit code is 1 on error"
assert_exit_code 1 run_lf --output text get 99999

test_summary
