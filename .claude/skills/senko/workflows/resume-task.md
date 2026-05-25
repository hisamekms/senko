# Resume Task

Resume an `in_progress` task whose previous session was interrupted. The task's status does NOT change — only `assignee_session_id` and (optionally) `metadata` are refreshed, and a `TaskEvent::Resumed` is emitted.

## Pre-check

```bash
senko task get <id>
```

- Verify `status == in_progress`. If anything else, refuse:
  - `draft` → suggest `senko task publish <id>` then `/senko start <id>` (a fresh start, not resume).
  - `todo` → suggest `/senko start <id>` (or `senko task start <id>`) — resume is for tasks already started.
  - `completed` / `canceled` → already finished; refuse.
- If `branch` is empty (non-repo task), skip worktree handling and continue.

## Step 1: Build metadata

Run the metadata builder script to read `[workflow.task_resume].metadata_fields` from config:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_resume
```

Parse the JSON output (`{"resolved": {...}, "prompts": [...]}`):

- If `prompts` array is non-empty, ask the user each prompt question using `AskUserQuestion`. Merge user answers into `resolved`.
- If `resolved` is empty (no keys) after merging, do NOT pass `--metadata`.

## Step 2: Resume the task

Pre-hooks (workflow stage), then the resume CLI call, then post-hooks:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh task_resume pre
senko task resume <id> --session-id <current-session-id> [--metadata '<final-metadata-json>']
bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh task_resume post
```

Execute any commands printed by the emit-hooks calls in order. Omit `--metadata` if there are no fields to pass.

## Step 3: Reuse branch / worktree

Resume reuses existing resources from the prior session. Generate the per-task branch-setup instructions in `resume` mode and follow them verbatim:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/generate-branch-setup.sh <id> --mode resume
```

The script reads `workflow.branch_mode` (table form `{type, create}` or legacy string) plus the task's `branch` field. In resume mode, even when `create=true` the instructions prefer reusing the existing worktree/branch first, and only fall back to fresh creation after explicit user confirmation. `create=false` modes behave the same as in execute mode (no provisioning).

Run the printed instructions before proceeding to Step 4. If the script exits non-zero, stop and report the error to the user.

## Step 4: Resume work

Branch on the task's `plan` field (from the `senko task get <id>` output already loaded in Pre-check):

### If `task.plan` is non-empty (saved plan exists)

Skip `EnterPlanMode` — the previously-approved plan is being adopted. Tell the user in one short line that the saved plan from the prior session is being reused (no `AskUserQuestion`, no approval gate).

Before starting implementation, load context: review prior commits on the branch (`git log main..HEAD`) and re-check the task's unchecked `definition_of_done` items so you know what is actually left.

The saved plan already contains the **Pre-start**, **Finalization**, and **Post-completion** sections (they were embedded by `execute-task.md` Step 3 when the plan was first written). Apply the Finalization and Post-completion sections from `task.plan` on completion as usual; the Pre-start "save the plan" step is already done and can be skipped.

Continue with implementation per `execute-task.md` Step 3 onward.

### If `task.plan` is empty or null (no saved plan)

Use `EnterPlanMode` to resume the work. Investigate the prior commits on the branch (`git log main..HEAD`) and review the task's unchecked `definition_of_done` items so the plan reflects what is actually left.

Continue with implementation as in `execute-task.md` Step 3 onward. The Pre-start / Finalization / Post-completion sections from `generate-plan-sections.sh <id>` still apply on completion.

## Contract note recording

If the task has `contract_id` set, follow the same Contract note pattern documented in `execute-task.md` (load notes on entry; record decisions / pitfalls / completion-summary as work progresses).
