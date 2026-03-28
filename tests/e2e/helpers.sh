#!/usr/bin/env bash
# e2e test helpers for localflow CLI

set -euo pipefail

PASS_COUNT=0
FAIL_COUNT=0

# Setup isolated test environment with temp directory
setup_test_env() {
  TEST_DIR="$(mktemp -d)"
  TEST_PROJECT_ROOT="$TEST_DIR/project"
  mkdir -p "$TEST_PROJECT_ROOT"

  # Resolve binary path
  if [[ -z "${LOCALFLOW:-}" ]]; then
    LOCALFLOW="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/target/debug/localflow"
  fi

  if [[ ! -x "$LOCALFLOW" ]]; then
    echo "FATAL: localflow binary not found at $LOCALFLOW" >&2
    exit 1
  fi

  export TEST_DIR TEST_PROJECT_ROOT LOCALFLOW
}

# Cleanup temp directory
cleanup_test_env() {
  if [[ -n "${TEST_DIR:-}" && -d "$TEST_DIR" ]]; then
    rm -rf "$TEST_DIR"
  fi
}

# Run localflow with --project-root pointed at test directory
# Also sets --db-path to keep the database inside the test directory
run_lf() {
  "$LOCALFLOW" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.localflow/data.db" "$@"
}

# --- Assertion functions ---

assert_eq() {
  local expected="$1"
  local actual="$2"
  local message="${3:-assert_eq}"

  if [[ "$expected" == "$actual" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message"
    echo "    expected: $expected"
    echo "    actual:   $actual"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local message="${3:-assert_contains}"

  if [[ "$haystack" == *"$needle"* ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message"
    echo "    haystack does not contain needle"
    echo "    needle: $needle"
    echo "    haystack: $haystack"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_exit_code() {
  local expected="$1"
  shift
  local actual=0
  "$@" >/dev/null 2>&1 || actual=$?

  if [[ "$expected" -eq "$actual" ]]; then
    echo "  PASS: exit code $expected for: $*"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: exit code for: $*"
    echo "    expected: $expected"
    echo "    actual:   $actual"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

assert_json_field() {
  local json="$1"
  local jq_path="$2"
  local expected="$3"
  local message="${4:-assert_json_field $jq_path}"

  local actual
  actual="$(echo "$json" | jq -r "$jq_path")"

  if [[ "$expected" == "$actual" ]]; then
    echo "  PASS: $message"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  FAIL: $message"
    echo "    jq_path:  $jq_path"
    echo "    expected: $expected"
    echo "    actual:   $actual"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

# Wait for a condition to become true (polling with timeout)
# Usage: wait_for "description" timeout_seconds "condition_command"
wait_for() {
  local description="$1"
  local timeout="${2:-5}"
  local condition="$3"
  local attempts=$(( timeout * 5 ))  # 0.2s interval
  local i=0

  while ! eval "$condition" 2>/dev/null; do
    if [[ $i -ge $attempts ]]; then
      echo "  TIMEOUT: $description (${timeout}s)" >&2
      return 1
    fi
    sleep 0.2
    i=$((i + 1))
  done
}

# Print test summary and exit with 1 if any failures
test_summary() {
  local total=$((PASS_COUNT + FAIL_COUNT))
  echo ""
  echo "=== Test Summary ==="
  echo "  Total: $total"
  echo "  Pass:  $PASS_COUNT"
  echo "  Fail:  $FAIL_COUNT"
  echo "===================="

  if [[ "$FAIL_COUNT" -gt 0 ]]; then
    exit 1
  fi
}
