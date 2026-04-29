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

## Step 3: Reuse worktree

Use the `branch` field from `senko task get <id>` as the branch name. The worktree for this branch most likely already exists from the prior session — reuse it; do **not** recreate. Only create a fresh worktree (following the project's worktree convention) if it is genuinely missing, and confirm with the user before doing so.

## Step 4: Plan Mode

Use `EnterPlanMode` to resume the work. Investigate the prior commits on the branch (`git log main..HEAD`) and review the task's `plan` and unchecked `definition_of_done` items so the plan reflects what is actually left.

Continue with implementation as in `execute-task.md` Step 3 onward. The Pre-start / Finalization / Post-completion sections from `generate-plan-sections.sh <id>` still apply on completion.

## Contract note recording

If the task has `contract_id` set, follow the same Contract note pattern documented in `execute-task.md` (load notes on entry; record decisions / pitfalls / completion-summary as work progresses).
