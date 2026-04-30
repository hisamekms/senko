#!/usr/bin/env bash
# e2e test runner for senko CLI

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Parse flags
FAST_MODE=false
PARALLEL=${PARALLEL:-$(nproc 2>/dev/null || echo 4)}
BACKEND="sqlite"
PORT_SEED=""

for arg in "$@"; do
  case "$arg" in
    --fast) FAST_MODE=true ;;
    --parallel=*) PARALLEL="${arg#--parallel=}" ;;
    --backend=*) BACKEND="${arg#--backend=}" ;;
    --seed=*) PORT_SEED="${arg#--seed=}" ;;
  esac
done

# Generate random port seed if not specified
if [[ -z "$PORT_SEED" ]]; then
  PORT_SEED=$(( RANDOM % 40000 + 10000 ))
fi
export PORT_SEED

export SENKO="$PROJECT_ROOT/target/debug/senko"

# Test files that support the postgres backend
PG_SUPPORTED_TESTS=(
  "test_serve_api.sh"
  "test_api_keys.sh"
  "test_metadata_fields_api.sh"
  "test_user_api.sh"
  "test_list_filter.sh"
  "test_dev_seed.sh"
)

is_pg_supported() {
  local name="$1"
  for supported in "${PG_SUPPORTED_TESTS[@]}"; do
    if [[ "$name" == "$supported" ]]; then
      return 0
    fi
  done
  return 1
}

# Collect test files
TEST_FILES=()
SKIPPED=0
for test_file in "$SCRIPT_DIR"/test_*.sh; do
  test_name="$(basename "$test_file")"
  if [[ "$FAST_MODE" == true ]] && [[ "$test_name" =~ ^test_watch ]]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi
  if [[ "$BACKEND" == "postgres" ]] && ! is_pg_supported "$test_name"; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi
  TEST_FILES+=("$test_file")
done

# Create temp directory for results
RESULTS_DIR="$(mktemp -d)"

# Cleanup function
PG_SERVER_PID=""
cleanup() {
  if [[ -n "$PG_SERVER_PID" ]]; then
    kill "$PG_SERVER_PID" 2>/dev/null || true
    wait "$PG_SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$RESULTS_DIR"
}
trap cleanup EXIT

# Start embedded PostgreSQL if needed
if [[ "$BACKEND" == "postgres" ]]; then
  PG_TEST_SERVER="$PROJECT_ROOT/target/debug/examples/pg_test_server"
  if [[ ! -x "$PG_TEST_SERVER" ]]; then
    echo "FATAL: pg_test_server not found at $PG_TEST_SERVER" >&2
    echo "  Build with: cargo build --example pg_test_server --features postgres" >&2
    exit 1
  fi

  PG_URL_FILE="$RESULTS_DIR/pg_url"
  DB_COUNT=${#TEST_FILES[@]}

  echo "=== Starting embedded PostgreSQL (creating $DB_COUNT databases) ==="
  "$PG_TEST_SERVER" "$PG_URL_FILE" "$DB_COUNT" &
  PG_SERVER_PID=$!

  # Wait for URL file to be written (server ready)
  attempts=0
  while [[ ! -s "$PG_URL_FILE" ]]; do
    if ! kill -0 "$PG_SERVER_PID" 2>/dev/null; then
      echo "FATAL: pg_test_server exited unexpectedly" >&2
      exit 1
    fi
    if [[ $attempts -ge 600 ]]; then
      echo "FATAL: pg_test_server failed to start within 120s" >&2
      exit 1
    fi
    sleep 0.2
    attempts=$((attempts + 1))
  done

  SENKO_TEST_PG_URL_PREFIX=$(cat "$PG_URL_FILE")
  export SENKO_TEST_BACKEND="postgres"
  export SENKO_TEST_PG_URL_PREFIX

  echo "=== PostgreSQL ready (seed=$PORT_SEED) ==="
fi

# Run a single test and record result
run_single_test() {
  local test_index="$1"
  local test_file="$2"
  local test_name
  test_name="$(basename "$test_file")"
  local result_file="$RESULTS_DIR/$test_name"

  if TEST_INDEX="$test_index" bash "$test_file" >"$result_file.out" 2>&1; then
    echo "ok" > "$result_file.status"
  else
    echo "fail" > "$result_file.status"
  fi
}

export -f run_single_test
export RESULTS_DIR SENKO

echo "=== Running ${#TEST_FILES[@]} tests (backend=$BACKEND, parallel=$PARALLEL, seed=$PORT_SEED) ==="
echo ""

# Run tests in parallel (pass "index filepath" pairs for deterministic port allocation)
printf '%s\n' "${TEST_FILES[@]}" | awk '{print NR-1, $0}' | xargs -P "$PARALLEL" -n 2 bash -c 'run_single_test "$@"' _

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
echo "  Backend: $BACKEND"
echo "  Test files run: ${#TEST_FILES[@]}"
[[ "$SKIPPED" -gt 0 ]] && echo "  Skipped: $SKIPPED"
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
