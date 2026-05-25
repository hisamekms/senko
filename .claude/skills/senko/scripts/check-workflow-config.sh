#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SENKO_BIN="${SENKO_BIN:-senko}"
ERRORS=0

CONFIG_JSON=$("$SENKO_BIN" config)

error() {
  echo "ERROR: $1" >&2
  ERRORS=$((ERRORS + 1))
}

warn() {
  echo "WARNING: $1" >&2
}

# Validate merge_via
MERGE_VIA=$(echo "$CONFIG_JSON" | jq -r '.workflow.merge_via')
case "$MERGE_VIA" in
  direct|pr) ;;
  *) error "invalid merge_via: $MERGE_VIA (expected: direct, pr)" ;;
esac

# Validate branch_mode (accept both legacy string form and modern table form).
BRANCH_MODE_TYPE=$(echo "$CONFIG_JSON" | jq -r '.workflow.branch_mode | if type == "string" then . else .type end')
BRANCH_MODE_CREATE=$(echo "$CONFIG_JSON" | jq -r '.workflow.branch_mode | if type == "string" then true else .create end')
case "$BRANCH_MODE_TYPE" in
  worktree|branch) ;;
  *) error "invalid branch_mode.type: $BRANCH_MODE_TYPE (expected: worktree, branch)" ;;
esac
case "$BRANCH_MODE_CREATE" in
  true|false) ;;
  *) error "invalid branch_mode.create: $BRANCH_MODE_CREATE (expected: true, false)" ;;
esac

# Validate merge_strategy
MERGE_STRATEGY=$(echo "$CONFIG_JSON" | jq -r '.workflow.merge_strategy')
case "$MERGE_STRATEGY" in
  rebase|squash) ;;
  *) error "invalid merge_strategy: $MERGE_STRATEGY (expected: rebase, squash)" ;;
esac

# Validate auto_merge is boolean
AUTO_MERGE=$(echo "$CONFIG_JSON" | jq -r '.workflow.auto_merge')
case "$AUTO_MERGE" in
  true|false) ;;
  *) error "invalid auto_merge: $AUTO_MERGE (expected: true, false)" ;;
esac

# Validate workflow stage hooks. Under the new schema each stage has a
# `hooks` map where each entry is a HookDef object with optional fields
# `command`, `prompt`, `when`, `mode`, `on_failure`, `enabled`.
# Stages known to be consumed by the skill (user-defined stages are allowed
# but unchecked here).
STAGES="task_add task_publish task_start task_resume task_complete task_cancel task_select plan implement branch_set branch_cleanup branch_merge pr_create pr_update contract_add contract_edit contract_delete contract_dod_check contract_dod_uncheck contract_note_add"

validate_hook() {
  local stage="$1" name="$2"
  local hook
  hook=$(echo "$CONFIG_JSON" | jq -c --arg s "$stage" --arg n "$name" '.workflow[$s].hooks[$n]')
  local hook_type
  hook_type=$(echo "$hook" | jq -r 'type')
  if [ "$hook_type" != "object" ]; then
    error "workflow.${stage}.hooks.${name}: invalid hook type '${hook_type}' (expected: object)"
    return
  fi
  local cmd prompt
  cmd=$(echo "$hook" | jq -r '.command // empty')
  prompt=$(echo "$hook" | jq -r '.prompt // empty')
  if [ -z "$cmd" ] && [ -z "$prompt" ]; then
    error "workflow.${stage}.hooks.${name}: hook requires 'command' or 'prompt' field"
  fi
  local when
  when=$(echo "$hook" | jq -r '.when // "post"')
  case "$when" in
    pre|post) ;;
    *) error "workflow.${stage}.hooks.${name}: invalid when '${when}' (expected: pre, post)" ;;
  esac
  local mode
  mode=$(echo "$hook" | jq -r '.mode // "async"')
  case "$mode" in
    sync|async) ;;
    *) error "workflow.${stage}.hooks.${name}: invalid mode '${mode}' (expected: sync, async)" ;;
  esac
  local on_failure
  on_failure=$(echo "$hook" | jq -r '.on_failure // "abort"')
  case "$on_failure" in
    abort|warn|ignore) ;;
    *) error "workflow.${stage}.hooks.${name}: invalid on_failure '${on_failure}' (expected: abort, warn, ignore)" ;;
  esac
}

for stage in $STAGES; do
  names=$(echo "$CONFIG_JSON" | jq -r --arg s "$stage" '.workflow[$s].hooks // {} | keys[]')
  while IFS= read -r name; do
    [ -z "$name" ] && continue
    validate_hook "$stage" "$name"
  done <<<"$names"
done

# Validate referenced merge scripts exist
if [ "$MERGE_VIA" = "direct" ]; then
  if [ "$MERGE_STRATEGY" = "rebase" ] && [ ! -f "$SCRIPT_DIR/rebase-merge.sh" ]; then
    error "rebase-merge.sh not found in $SCRIPT_DIR"
  fi
  if [ "$MERGE_STRATEGY" = "squash" ] && [ ! -f "$SCRIPT_DIR/squash-merge.sh" ]; then
    error "squash-merge.sh not found in $SCRIPT_DIR"
  fi
fi

# Result
if [ "$ERRORS" -gt 0 ]; then
  echo "Validation failed with $ERRORS error(s)" >&2
  exit 1
else
  echo "Workflow configuration is valid"
fi
