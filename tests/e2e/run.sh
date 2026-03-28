#!/usr/bin/env bash
# e2e test runner for senko CLI

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Parse flags
FAST_MODE=false
PARALLEL=${PARALLEL:-$(nproc 2>/dev/null || echo 4)}

for arg in "$@"; do
  case "$arg" in
    --fast) FAST_MODE=true ;;
    --parallel=*) PARALLEL="${arg#--parallel=}" ;;
  esac
done

echo "=== Building senko ==="
cd "$PROJECT_ROOT"
cargo build 2>&1
echo ""

export SENKO="$PROJECT_ROOT/target/debug/senko"

# Collect test files
TEST_FILES=()
SKIPPED=0
for test_file in "$SCRIPT_DIR"/test_*.sh; do
  test_name="$(basename "$test_file")"
  if [[ "$FAST_MODE" == true ]] && [[ "$test_name" =~ ^test_watch ]]; then
    echo "=== Skipping: $test_name (fast mode) ==="
    SKIPPED=$((SKIPPED + 1))
    continue
  fi
  TEST_FILES+=("$test_file")
done

# Create temp directory for results
RESULTS_DIR="$(mktemp -d)"
trap "rm -rf '$RESULTS_DIR'" EXIT

# Run a single test and record result
run_single_test() {
  local test_file="$1"
  local test_name
  test_name="$(basename "$test_file")"
  local result_file="$RESULTS_DIR/$test_name"

  if bash "$test_file" >"$result_file.out" 2>&1; then
    echo "ok" > "$result_file.status"
  else
    echo "fail" > "$result_file.status"
  fi
}

export -f run_single_test
export RESULTS_DIR SENKO

echo "=== Running ${#TEST_FILES[@]} tests (parallel=$PARALLEL) ==="
echo ""

# Run tests in parallel
printf '%s\n' "${TEST_FILES[@]}" | xargs -P "$PARALLEL" -I {} bash -c 'run_single_test "$@"' _ {}

# Collect results
FAILED_TESTS=()
for test_file in "${TEST_FILES[@]}"; do
  test_name="$(basename "$test_file")"
  result_file="$RESULTS_DIR/$test_name"

  echo "=== $test_name ==="
  cat "$result_file.out" 2>/dev/null || true

  if [[ "$(cat "$result_file.status" 2>/dev/null)" == "ok" ]]; then
    echo ">>> $test_name: OK"
  else
    echo ">>> $test_name: FAILED"
    FAILED_TESTS+=("$test_name")
  fi
  echo ""
done

echo "=== Overall Results ==="
echo "  Test files run: ${#TEST_FILES[@]}"
[[ "$SKIPPED" -gt 0 ]] && echo "  Skipped: $SKIPPED (fast mode)"
echo "  Failed: ${#FAILED_TESTS[@]}"

if [[ ${#FAILED_TESTS[@]} -gt 0 ]]; then
  echo "  Failed tests:"
  for t in "${FAILED_TESTS[@]}"; do
    echo "    - $t"
  done
  exit 1
else
  echo "  All tests passed!"
  exit 0
fi
