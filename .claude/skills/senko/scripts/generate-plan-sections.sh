#!/usr/bin/env bash
set -euo pipefail

TASK_ID="${1:?Usage: generate-plan-sections.sh <task-id>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SENKO_BIN="${SENKO_BIN:-senko}"

CONFIG_JSON=$("$SENKO_BIN" config)
MERGE_VIA=$(echo "$CONFIG_JSON" | jq -r '.workflow.merge_via')
AUTO_MERGE=$(echo "$CONFIG_JSON" | jq -r '.workflow.auto_merge')
BRANCH_MODE=$(echo "$CONFIG_JSON" | jq -r '.workflow.branch_mode')
MERGE_STRATEGY=$(echo "$CONFIG_JSON" | jq -r '.workflow.merge_strategy')

# Emit instructions for a workflow stage as bullet points.
# Usage: emit_instructions <stage>
emit_instructions() {
  local stage="$1"
  local count
  count=$(echo "$CONFIG_JSON" | jq --arg s "$stage" '.workflow[$s].instructions // [] | length')
  for i in $(seq 0 $((count - 1))); do
    local text
    text=$(echo "$CONFIG_JSON" | jq -r --arg s "$stage" ".workflow[\$s].instructions[$i]")
    echo "- ${text}"
  done
}

# Emit hooks for a workflow stage matching the given `when` phase.
# Usage: emit_hooks <stage> <pre|post>
# Hooks live under `.workflow[stage].hooks` as a map of HookDef objects, each
# with optional fields: command, prompt, when ("pre" | "post"; default "post"),
# on_failure, enabled (default true). Disabled or phase-mismatched hooks are skipped.
emit_hooks() {
  local stage="$1"
  local phase="$2"
  local names
  names=$(echo "$CONFIG_JSON" | jq -r --arg s "$stage" '.workflow[$s].hooks // {} | keys[]')
  while IFS= read -r name; do
    [ -z "$name" ] && continue
    local hook enabled when cmd prompt
    hook=$(echo "$CONFIG_JSON" | jq -c --arg s "$stage" --arg n "$name" '.workflow[$s].hooks[$n]')
    enabled=$(echo "$hook" | jq -r '.enabled != false')
    when=$(echo "$hook" | jq -r '.when // "post"')
    [ "$enabled" = "false" ] && continue
    [ "$when" != "$phase" ] && continue
    cmd=$(echo "$hook" | jq -r '.command // empty')
    prompt=$(echo "$hook" | jq -r '.prompt // empty')
    if [ -n "$cmd" ]; then
      echo "- Run: \`${cmd}\`"
    fi
    if [ -n "$prompt" ]; then
      echo "- ${prompt}"
    fi
  done <<<"$names"
}

# Emit metadata collection step for a workflow stage if metadata_fields is configured.
# Usage: emit_metadata_step <stage> <task-id>
emit_metadata_step() {
  local stage="$1"
  local task_id="$2"
  local count
  count=$(echo "$CONFIG_JSON" | jq --arg s "$stage" '.workflow[$s].metadata_fields // [] | length')
  if [ "$count" -gt 0 ]; then
    echo "- Collect metadata for \`${stage}\` stage:"
    echo "  1. Run \`bash \${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh ${stage}\`"
    echo "  2. If \`prompts\` array is non-empty, ask the user each prompt using \`AskUserQuestion\`"
    echo "  3. Merge answers into \`resolved\`, then shallow-merge into existing task metadata"
    echo "  4. Save: \`senko task edit ${task_id} --metadata '<merged-json>'\`"
  fi
}

# --- Pre-start ---
cat <<EOF
# Pre-start
- Save this plan to the task:
  1. Write the full approved plan text to a temporary file (e.g., \`/tmp/senko-plan-${TASK_ID}.md\`)
  2. Run \`senko task edit ${TASK_ID} --plan-file /tmp/senko-plan-${TASK_ID}.md\`
  3. Delete the temporary file
- This must be done before starting implementation.
EOF

emit_metadata_step "implement" "$TASK_ID"
emit_instructions "implement"
emit_hooks "implement" "pre"

# --- Finalization ---
cat <<'HEADER'

# Finalization
- When implementation is done, verify DoD items using the dod-verifier subagent:
  1. Run `senko task get <id>` and check `definition_of_done` for unchecked items
  2. Launch the `dod-verifier` agent (via Agent tool) with the task ID and unchecked DoD items
  3. Process the subagent's results for each item:
     - **VERIFIED**: `senko task dod check <id> <index>`
     - **NEEDS_USER_APPROVAL**: Use `AskUserQuestion` to confirm with the user, then check if approved
     - **NOT_ACHIEVED**: Go back and implement the missing item, then re-verify
  4. All DoD items must be checked before proceeding
HEADER

emit_metadata_step "branch_merge" "$TASK_ID"
emit_hooks "branch_merge" "pre"
emit_instructions "branch_merge"

# --- Merge/PR step ---
if [ "$MERGE_VIA" = "direct" ]; then
  if [ "$AUTO_MERGE" != "true" ]; then
    echo "- **MANDATORY**: Use \`AskUserQuestion\` to ask the user for completion approval before proceeding to merge. Do NOT skip this step. Wait for explicit user approval."
  fi
  if [ "$MERGE_STRATEGY" = "squash" ]; then
    echo "- Squash merge the branch into main (all DoD items must be checked before this step):"
    echo "  \`bash \${CLAUDE_SKILL_DIR}/scripts/squash-merge.sh <branch-name>\`"
  else
    echo "- Rebase merge the branch into main using the rebase-merge script (all DoD items must be checked before this step):"
    echo "  \`bash \${CLAUDE_SKILL_DIR}/scripts/rebase-merge.sh <branch-name>\`"
  fi
  echo "- If the merge script exits with code 10 (primary worktree has uncommitted changes), use \`AskUserQuestion\` to inform the user and ask them to clean up the primary worktree before retrying"
  echo "- If the merge script exits with code 11 (rebase conflict), use \`AskUserQuestion\` to inform the user about the conflict and ask how to proceed (manual resolution or abort)"
elif [ "$MERGE_VIA" = "pr" ]; then
  cat <<PREOF
- Create PR (all DoD items must be checked before this step)
- After creating the PR, save the PR URL: \`senko task edit ${TASK_ID} --pr-url <pr_url>\`
- Begin PR polling loop:
  1. Run \`gh pr view <pr_url> --json state,reviews,comments\`
  2. If there are new review comments or requested changes:
     - Address each review comment (fix code, respond to feedback)
     - Push the changes and continue polling
  3. If the PR state is MERGED:
     - Exit the polling loop and proceed to completion
  4. Otherwise, wait 1 minute and repeat from step 1
PREOF
fi

emit_hooks "branch_merge" "post"

# --- Complete step ---
emit_metadata_step "task_complete" "$TASK_ID"
emit_hooks "task_complete" "pre"
emit_instructions "task_complete"

if [ "$MERGE_VIA" = "pr" ]; then
  echo "- **Do NOT run \`senko task complete\` in this workflow.** Task completion for PR-based workflows is handled separately via \`/senko complete ${TASK_ID}\`."
else
  echo "- Complete the task: \`senko task complete ${TASK_ID}\`"
fi

# --- Post-completion ---
echo ""
echo "# Post-completion"

emit_metadata_step "branch_cleanup" "$TASK_ID"
emit_hooks "branch_cleanup" "pre"
emit_instructions "branch_cleanup"

if [ "$BRANCH_MODE" = "worktree" ]; then
  echo "- Delete the worktree (following the project's worktree-removal procedure)"
fi

emit_hooks "branch_cleanup" "post"
