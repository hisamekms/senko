#!/usr/bin/env bash
# e2e test: generate-branch-setup.sh emits config-driven instructions
# for the four (branch_mode.type × branch_mode.create) combinations,
# plus legacy string form and the non-repo (branch=null) case.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: generate-branch-setup.sh ---"

PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$PROJECT_ROOT/.claude/skills/senko/scripts/generate-branch-setup.sh"
SENKO_DIR="$TEST_PROJECT_ROOT/.senko"
mkdir -p "$SENKO_DIR"

# Shim that forwards `senko ...` to `$SENKO --project-root ... --db-path ...`
# so the script's internal `$SENKO_BIN` calls reach the isolated test backend.
# Build the shim with printf %q so multi-arg backend flags (--db-path <path>,
# --postgres-url <url>) are preserved as separate arguments.
SHIM="$TEST_DIR/senko-shim"
{
  echo '#!/usr/bin/env bash'
  printf 'exec %q' "$SENKO"
  printf ' %q' "--project-root" "$TEST_PROJECT_ROOT"
  for arg in "${SENKO_DB_ARGS[@]}"; do
    printf ' %q' "$arg"
  done
  printf ' "$@"\n'
} >"$SHIM"
chmod +x "$SHIM"
export SENKO_BIN="$SHIM"

# Create one task with a branch and one without (for the non-repo case).
TASK_WITH_BRANCH="$(run_lf --output json task add --title "task with branch" --branch "feature/x" | jq -r '.id')"
TASK_NO_BRANCH="$(run_lf --output json task add --title "task no branch" | jq -r '.id')"

write_config() {
  # $1 = TOML snippet under [workflow]
  cat >"$SENKO_DIR/config.toml" <<EOF
[workflow]
$1
EOF
}

# 1. Default config (no config.toml): worktree × create=true
echo "[1] Default config (worktree × create=true)"
rm -f "$SENKO_DIR/config.toml"
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH")"
assert_contains "$OUT" 'type = "worktree", create = true' "default → worktree×create=true header"
assert_contains "$OUT" "create a fresh worktree for branch \`feature/x\`" "default → create-worktree instruction"

# 2. branch × create=false (most distinct case from default)
echo "[2] Config { type=branch, create=false }"
write_config '[workflow.branch_mode]
type = "branch"
create = false'
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH")"
assert_contains "$OUT" 'type = "branch", create = false' "branch×create=false header"
assert_contains "$OUT" "currently checked-out branch" "branch×create=false → no switch"
assert_contains "$OUT" "Do NOT switch branches" "branch×create=false → explicit no-switch"

# 3. worktree × create=false → reuse existing or ERROR
echo "[3] Config { type=worktree, create=false }"
write_config '[workflow.branch_mode]
type = "worktree"
create = false'
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH")"
assert_contains "$OUT" 'type = "worktree", create = false' "worktree×create=false header"
assert_contains "$OUT" "externally-managed worktree" "worktree×create=false → externally managed"
assert_contains "$OUT" "Do NOT create" "worktree×create=false → no fallback"
assert_contains "$OUT" "STOP and report" "worktree×create=false → stop on missing"

# 4. branch × create=true
echo "[4] Config { type=branch, create=true }"
write_config '[workflow.branch_mode]
type = "branch"
create = true'
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH")"
assert_contains "$OUT" 'type = "branch", create = true' "branch×create=true header"
assert_contains "$OUT" "git switch feature/x" "branch×create=true → switch existing"
assert_contains "$OUT" "git switch -c feature/x" "branch×create=true → create if missing"

# 5. Legacy string form: branch_mode = "worktree" → equivalent to {type=worktree, create=true}
echo "[5] Legacy string form (branch_mode = \"worktree\")"
write_config 'branch_mode = "worktree"'
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH")"
assert_contains "$OUT" 'type = "worktree", create = true' "legacy 'worktree' → worktree×create=true"
assert_contains "$OUT" "create a fresh worktree for branch \`feature/x\`" "legacy 'worktree' → same as modern default"

# 6. resume mode for worktree×create=true → reuse-first language
echo "[6] --mode resume (worktree × create=true)"
rm -f "$SENKO_DIR/config.toml"
OUT="$(bash "$SCRIPT" "$TASK_WITH_BRANCH" --mode resume)"
assert_contains "$OUT" "resume mode" "resume → resume-mode header"
assert_contains "$OUT" "reuse the existing worktree" "resume → reuse-first language"
assert_contains "$OUT" "AskUserQuestion" "resume → confirm before fresh create"

# 7. Non-repo task (branch=null) → skip message regardless of config
echo "[7] Non-repo task (branch=null)"
OUT="$(bash "$SCRIPT" "$TASK_NO_BRANCH")"
assert_contains "$OUT" "non-repo task" "branch=null → skip message"

# 8. check-workflow-config.sh accepts legacy and modern branch_mode
echo "[8] check-workflow-config.sh accepts both forms"
CHECK_SCRIPT="$PROJECT_ROOT/.claude/skills/senko/scripts/check-workflow-config.sh"

write_config 'branch_mode = "branch"'
OUT="$(bash "$CHECK_SCRIPT" 2>&1)"
assert_contains "$OUT" "Workflow configuration is valid" "legacy branch_mode='branch' → valid"

write_config '[workflow.branch_mode]
type = "branch"
create = false'
OUT="$(bash "$CHECK_SCRIPT" 2>&1)"
assert_contains "$OUT" "Workflow configuration is valid" "modern {type=branch, create=false} → valid"

test_summary
