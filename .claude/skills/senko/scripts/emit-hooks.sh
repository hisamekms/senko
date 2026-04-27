#!/usr/bin/env bash
set -euo pipefail

# Emit workflow-stage hooks as plan-instruction bullets, for a single stage and
# phase. This script is called ad-hoc from workflow markdown files (e.g.
# around `senko contract add` calls) so that pre/post hooks can be expanded
# inline, outside the task-lifecycle sections handled by
# generate-plan-sections.sh.
#
# Usage: emit-hooks.sh <stage> <pre|post>
#   stage: task_add, task_publish, task_start, task_resume, task_complete, task_cancel,
#          task_select, branch_set, branch_cleanup, branch_merge, pr_create,
#          pr_update, plan, implement,
#          contract_add, contract_edit, contract_delete, contract_dod_check,
#          contract_dod_uncheck, contract_note_add
#   phase: pre | post
#
# Hooks live under `.workflow[stage].hooks` as a map of HookDef objects, each
# with optional fields: command, prompt, when ("pre" | "post"; default "post"),
# on_failure, enabled (default true). Disabled or phase-mismatched hooks are
# skipped. Each matching hook yields one bullet on stdout:
#   - Run: `<command>`
#   - <prompt>
# If no hooks match, nothing is printed.

STAGE="${1:?Usage: emit-hooks.sh <stage> <pre|post>}"
PHASE="${2:?Usage: emit-hooks.sh <stage> <pre|post>}"

case "$PHASE" in
  pre|post) ;;
  *)
    echo "emit-hooks.sh: invalid phase '$PHASE' (expected: pre, post)" >&2
    exit 2
    ;;
esac

SENKO_BIN="${SENKO_BIN:-senko}"
CONFIG_JSON=$("$SENKO_BIN" config)

names=$(echo "$CONFIG_JSON" | jq -r --arg s "$STAGE" '.workflow[$s].hooks // {} | keys[]')

while IFS= read -r name; do
  [ -z "$name" ] && continue
  hook=$(echo "$CONFIG_JSON" | jq -c --arg s "$STAGE" --arg n "$name" '.workflow[$s].hooks[$n]')
  enabled=$(echo "$hook" | jq -r '.enabled != false')
  when=$(echo "$hook" | jq -r '.when // "post"')
  [ "$enabled" = "false" ] && continue
  [ "$when" != "$PHASE" ] && continue
  cmd=$(echo "$hook" | jq -r '.command // empty')
  prompt=$(echo "$hook" | jq -r '.prompt // empty')
  if [ -n "$cmd" ]; then
    echo "- Run: \`${cmd}\`"
  fi
  if [ -n "$prompt" ]; then
    echo "- ${prompt}"
  fi
done <<<"$names"
