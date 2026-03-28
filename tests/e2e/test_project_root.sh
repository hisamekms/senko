#!/usr/bin/env bash
# e2e tests for project root detection

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "=== Project Root Detection Tests ==="

# --- Test 1: --project-root explicit flag ---
echo ""
echo "--- Test 1: --project-root で明示的にルート指定 ---"

result=$(run_lf --output json add --title "explicit root task")
assert_json_field "$result" ".title" "explicit root task" "task created with explicit --project-root"

# Verify data.db was created at the explicit --db-path location
if [[ -f "$TEST_PROJECT_ROOT/.localflow/data.db" ]]; then
  echo "  PASS: data.db exists at explicit project root"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $TEST_PROJECT_ROOT/.localflow/data.db"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# --- Test 2: .localflow/ ディレクトリがある場合の自動検出 ---
echo ""
echo "--- Test 2: .localflow/ ディレクトリによる自動検出 ---"

AUTO_ROOT="$TEST_DIR/auto_localflow"
AUTO_DB="$AUTO_ROOT/.localflow/data.db"
mkdir -p "$AUTO_ROOT/.localflow"

result=$(cd "$AUTO_ROOT" && "$LOCALFLOW" --output json --db-path "$AUTO_DB" add --title "auto detected task")
assert_json_field "$result" ".title" "auto detected task" "task created via .localflow/ auto-detection"

if [[ -f "$AUTO_DB" ]]; then
  echo "  PASS: data.db exists at auto-detected root"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $AUTO_DB"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# --- Test 3: .git/ ディレクトリがある場合のフォールバック ---
echo ""
echo "--- Test 3: .git/ ディレクトリによるフォールバック ---"

GIT_ROOT="$TEST_DIR/git_fallback"
GIT_DB="$GIT_ROOT/.localflow/data.db"
mkdir -p "$GIT_ROOT/.git"

result=$(cd "$GIT_ROOT" && "$LOCALFLOW" --output json --db-path "$GIT_DB" add --title "git fallback task")
assert_json_field "$result" ".title" "git fallback task" "task created via .git/ fallback"

if [[ -f "$GIT_DB" ]]; then
  echo "  PASS: data.db exists at git-based root"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $GIT_DB"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# --- Test 4: サブディレクトリから実行した場合の上方探索 ---
echo ""
echo "--- Test 4: サブディレクトリからの上方探索 ---"

PARENT_ROOT="$TEST_DIR/parent_root"
PARENT_DB="$PARENT_ROOT/.localflow/data.db"
mkdir -p "$PARENT_ROOT/.localflow"
SUBDIR="$PARENT_ROOT/sub/deep/nested"
mkdir -p "$SUBDIR"

result=$(cd "$SUBDIR" && "$LOCALFLOW" --output json --db-path "$PARENT_DB" add --title "upward search task")
assert_json_field "$result" ".title" "upward search task" "task created via upward search from subdirectory"

if [[ -f "$PARENT_DB" ]]; then
  echo "  PASS: data.db exists at parent root (not subdirectory)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $PARENT_DB"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Verify data.db was NOT created in the subdirectory
if [[ ! -f "$SUBDIR/.localflow/data.db" ]]; then
  echo "  PASS: data.db not created in subdirectory"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db unexpectedly created in subdirectory"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# --- Test 5: XDGデフォルトパスのテスト ---
echo ""
echo "--- Test 5: XDGデフォルトパス ---"

XDG_ROOT="$TEST_DIR/xdg_test"
XDG_DATA="$TEST_DIR/xdg_data"
mkdir -p "$XDG_ROOT/.localflow"

result=$(cd "$XDG_ROOT" && XDG_DATA_HOME="$XDG_DATA" "$LOCALFLOW" --output json add --title "xdg default task")
assert_json_field "$result" ".title" "xdg default task" "task created with XDG default path"

if [[ -f "$XDG_DATA/localflow/data.db" ]]; then
  echo "  PASS: data.db exists at XDG_DATA_HOME/localflow/data.db"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $XDG_DATA/localflow/data.db"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# --- Test 6: --db-path overrides XDG default ---
echo ""
echo "--- Test 6: --db-path がXDGデフォルトを上書き ---"

OVERRIDE_ROOT="$TEST_DIR/override_test"
OVERRIDE_DB="$TEST_DIR/custom_db.db"
mkdir -p "$OVERRIDE_ROOT/.localflow"

result=$(cd "$OVERRIDE_ROOT" && "$LOCALFLOW" --output json --db-path "$OVERRIDE_DB" add --title "override task")
assert_json_field "$result" ".title" "override task" "task created with --db-path override"

if [[ -f "$OVERRIDE_DB" ]]; then
  echo "  PASS: data.db exists at custom --db-path"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: data.db not found at $OVERRIDE_DB"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
