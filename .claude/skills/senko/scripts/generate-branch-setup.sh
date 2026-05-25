#!/usr/bin/env bash
# Emit per-task branch-setup instructions (Markdown bullets) for the agent
# to follow. Reads `.workflow.branch_mode` from `senko config` and the
# task's `.branch` field, then prints the right instructions for the
# combination of (branch_mode.type, branch_mode.create, mode).
#
# Usage: generate-branch-setup.sh <task-id> [--mode execute|resume]
#
# branch_mode is accepted in both forms:
#   - legacy string: "worktree" | "branch"      (treated as { type=<value>, create=true })
#   - modern table:  { "type": ..., "create": ... }
#
# Exit codes:
#   0  ok (instructions printed to stdout)
#   1  invalid branch_mode value (error printed to stderr)
#   2  invalid CLI args
set -euo pipefail

TASK_ID="${1:?Usage: generate-branch-setup.sh <task-id> [--mode execute|resume]}"
shift

MODE="execute"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:?--mode requires a value (execute|resume)}"
      shift 2
      ;;
    --mode=*)
      MODE="${1#--mode=}"
      shift
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

case "$MODE" in
  execute|resume) ;;
  *) echo "ERROR: invalid --mode: $MODE (expected: execute, resume)" >&2; exit 2 ;;
esac

SENKO_BIN="${SENKO_BIN:-senko}"

CONFIG_JSON=$("$SENKO_BIN" config)
TASK_JSON=$("$SENKO_BIN" task get "$TASK_ID")

BRANCH=$(echo "$TASK_JSON" | jq -r '.branch // empty')

if [ -z "$BRANCH" ]; then
  echo "- Task has no \`branch\` set (non-repo task). Skip branch / worktree setup and proceed."
  exit 0
fi

# Read branch_mode supporting both legacy string form and modern table form.
BM_TYPE=$(echo "$CONFIG_JSON" | jq -r '.workflow.branch_mode | if type == "string" then . else .type end')
BM_CREATE=$(echo "$CONFIG_JSON" | jq -r '.workflow.branch_mode | if type == "string" then true else .create end')

case "$BM_TYPE" in
  worktree|branch) ;;
  *) echo "ERROR: invalid workflow.branch_mode.type: $BM_TYPE (expected: worktree, branch)" >&2; exit 1 ;;
esac

case "$BM_CREATE" in
  true|false) ;;
  *) echo "ERROR: invalid workflow.branch_mode.create: $BM_CREATE (expected: true, false)" >&2; exit 1 ;;
esac

case "$BM_TYPE/$BM_CREATE/$MODE" in
  worktree/true/execute)
    cat <<EOF
- branch_mode = { type = "worktree", create = true }: create a fresh worktree for branch \`${BRANCH}\` following the project's worktree convention.
- Switch into the new worktree directory before continuing.
EOF
    ;;
  worktree/true/resume)
    cat <<EOF
- branch_mode = { type = "worktree", create = true } (resume mode): reuse the existing worktree for branch \`${BRANCH}\` from the prior session.
- If that worktree is missing, confirm with the user via \`AskUserQuestion\` before creating a fresh one (resume should not silently provision new resources).
- Once located (or freshly created with user approval), switch into the worktree directory before continuing.
EOF
    ;;
  worktree/false/execute|worktree/false/resume)
    cat <<EOF
- branch_mode = { type = "worktree", create = false }: locate the externally-managed worktree for branch \`${BRANCH}\`. Do NOT create a new one.
- If no worktree for \`${BRANCH}\` exists, STOP and report the error to the user. Do not fall back to the current directory or any other branch.
- Switch into the existing worktree directory before continuing.
EOF
    ;;
  branch/true/execute)
    cat <<EOF
- branch_mode = { type = "branch", create = true }: ensure branch \`${BRANCH}\` is checked out in the current repository (no worktree).
  - If the branch already exists: \`git switch ${BRANCH}\`.
  - Otherwise: \`git switch -c ${BRANCH}\`.
- Continue working from the current repository checkout.
EOF
    ;;
  branch/true/resume)
    cat <<EOF
- branch_mode = { type = "branch", create = true } (resume mode): reuse branch \`${BRANCH}\` from the prior session. If it is not the current branch, run \`git switch ${BRANCH}\`.
- If the branch is missing, confirm with the user via \`AskUserQuestion\` before creating it fresh (resume should not silently provision new resources).
- Continue working from the current repository checkout (no worktree).
EOF
    ;;
  branch/false/execute|branch/false/resume)
    cat <<EOF
- branch_mode = { type = "branch", create = false }: work on the currently checked-out branch. Do NOT switch branches or create one — the task's \`branch\` field (\`${BRANCH}\`) is recorded for reference only.
- Continue working from the current repository checkout.
EOF
    ;;
esac
