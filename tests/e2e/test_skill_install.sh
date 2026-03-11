#!/usr/bin/env bash
# e2e test: skill-install command

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Skill Install ---"

# 1. Default path with --yes
echo "[1] Default path with --yes"
OUTPUT="$(run_lf skill-install --yes)"
SKILL_PATH="$TEST_PROJECT_ROOT/.claude/skills/localflow/SKILL.md"

if [[ -f "$SKILL_PATH" ]]; then
  echo "  PASS: SKILL.md created at default path"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: SKILL.md not found at $SKILL_PATH"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$OUTPUT" "SKILL.md written to" "output confirms file written"

# Clean up for next test
rm -rf "$TEST_PROJECT_ROOT/.claude"

# 2. --output-dir for custom path
echo "[2] Custom path with --output-dir"
CUSTOM_DIR="$TEST_DIR/custom-output"
mkdir -p "$CUSTOM_DIR"
OUTPUT="$(run_lf skill-install --output-dir "$CUSTOM_DIR")"
CUSTOM_SKILL="$CUSTOM_DIR/SKILL.md"

if [[ -f "$CUSTOM_SKILL" ]]; then
  echo "  PASS: SKILL.md created at custom path"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: SKILL.md not found at $CUSTOM_SKILL"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi
assert_contains "$OUTPUT" "SKILL.md written to" "output confirms custom path written"

# 3. --yes skips prompt when .claude/ does not exist
echo "[3] --yes skips confirmation prompt"
rm -rf "$TEST_PROJECT_ROOT/.claude"
# If prompt were shown without --yes, it would hang. Success = no hang.
OUTPUT="$(run_lf skill-install --yes)"
assert_contains "$OUTPUT" "SKILL.md written to" "--yes skips prompt successfully"
assert_contains "$OUTPUT" "Created .claude/ directory" "reports .claude/ directory creation"

# 4. .claude/ already exists → no prompt needed even without --yes
echo "[4] Existing .claude/ directory (no --yes needed)"
# .claude/ was created by test 3, remove SKILL.md but keep .claude/
rm -f "$TEST_PROJECT_ROOT/.claude/skills/localflow/SKILL.md"
OUTPUT="$(run_lf skill-install)"

if [[ -f "$TEST_PROJECT_ROOT/.claude/skills/localflow/SKILL.md" ]]; then
  echo "  PASS: SKILL.md created without --yes when .claude/ exists"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: SKILL.md not created when .claude/ already exists"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# 5. Generated file content contains command names
echo "[5] SKILL.md content verification"
CONTENT="$(cat "$TEST_PROJECT_ROOT/.claude/skills/localflow/SKILL.md")"
assert_contains "$CONTENT" "localflow add" "content contains 'localflow add'"
assert_contains "$CONTENT" "localflow list" "content contains 'localflow list'"
assert_contains "$CONTENT" "localflow next" "content contains 'localflow next'"

test_summary
