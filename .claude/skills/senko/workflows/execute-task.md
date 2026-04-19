# Execute Task

## Pre-check

> Skip this entire section if coming from `senko task next` (already validated — the selected task is already in `in_progress` with dependencies satisfied). Proceed directly to **Execution Steps**.

```bash
senko task get <id>
```

### Default (no `--force`)

- Verify `status` is **`todo`**. If it is anything else (`draft`, `in_progress`, `completed`, `canceled`), inform the user (include the actual status) and stop. For example:
  - `draft` → tell the user to run `senko task ready <id>` first.
  - `in_progress` → tell the user that work is already in progress; if they truly want to resume, rerun with `--force`.
  - `completed` / `canceled` → tell the user the task is already finished and refuse to proceed.
- Verify that **every** entry in `dependencies` has `status == completed`. If any dependency is still incomplete, list the offending dependency IDs / statuses and stop.
- On success, continue to **Build metadata** below, then start the task and move to **Execution Steps**.

### With `--force`

`--force` has one and only one purpose: **resume an `in_progress` task** whose work was interrupted in a previous session.

- If `status` is **not** `in_progress`, reject and stop — `--force` does NOT accept any other status:
  - `draft` → reject (ask the user to `senko task ready <id>` first, then rerun **without** `--force`).
  - `todo` → reject (rerun **without** `--force`; `--force` is not a way to start a fresh task).
  - `completed` / `canceled` → reject (the task is already finished; finished tasks cannot be re-opened with `--force`).
- `--force` is **not** a dependency bypass. It only changes the status check; dependencies are already assumed to have been validated when the task was first started.
- When `status == in_progress`:
  - Do **not** run the **Build metadata** step below — it belongs to the `todo → in_progress` transition and has already run in the prior session.
  - Do **not** run `senko task start <id>` — the task is already `in_progress`, so calling `start` again would fail / no-op.
  - Skip directly to **Execution Steps**. In Step 2, **reuse the existing worktree** for `branch` — it should already exist from the previous session. If it is genuinely missing, confirm with the user before creating a fresh one.
  - Continue into Step 3 (Plan Mode) to resume the work.

### Build metadata

> Only run this subsection on the **default** path (status was `todo`). Skip entirely on the `--force` path.

Run the metadata builder script to read `[workflow.task_start].metadata_fields` from config:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_start
```

Parse the JSON output (`{"resolved": {...}, "prompts": [...]}`):

- If `prompts` array is non-empty, ask the user each prompt question using `AskUserQuestion`. Merge user answers into `resolved`.
- If `resolved` is empty (no keys) after merging, do NOT pass `--metadata`.

Then transition:

```bash
senko task start <id> --metadata '<final-metadata-json>'
```

Omit `--metadata` entirely if there are no metadata fields to pass.

## Execution Steps

> **Terminal-task redirect**: if the task carries the `contract-terminal` tag, do NOT follow the rest of this file. Switch to `${CLAUDE_SKILL_DIR}/workflows/contract-terminal.md` — terminal tasks verify the linked Contract rather than planning and implementing code.

### Step 1: Review Task

Read full task info from `senko task get <id>` output: `description`, `plan`, `definition_of_done`, `in_scope`, `out_of_scope`, and `contract_id`.

**If `contract_id` is set (non-null)**, also load the Contract context — these notes are the shared memory across every sub-task that is linked to this Contract:

```bash
senko contract get <contract_id>
senko contract note list <contract_id>
```

Surface the Contract's title, description, DoD checklist, and the full note list into the assistant's working context before moving on. Prior sessions may have recorded decisions, gotchas, or scope clarifications there.

### Step 2: Create Worktree

Use the `branch` field from `senko task get <id>` as the branch name. If `branch` is not set (non-repo task), skip worktree creation and proceed to Step 3. Use the `/wth` skill to create a worktree.

> **`--force` resume**: If you entered via `--force` on an `in_progress` task, the worktree for this branch most likely already exists from the prior session. Reuse it — do not recreate it. Only create a fresh worktree if it is genuinely missing, and confirm with the user before doing so.

### Step 3: Plan Mode

Use `EnterPlanMode` to create an implementation plan. Investigate the codebase based on the task's description.

Before creating the plan, generate the workflow-specific sections by running:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/generate-plan-sections.sh <id>
```

The script outputs three sections: **Pre-start**, **Finalization**, and **Post-completion**. Include all three sections verbatim in the plan.

Wait for the user to approve the plan.

## Contract note recording

> This subsection applies **only** when the task has a `contract_id` set. Skip entirely for Contract-less tasks.

Notes are the shared memory between sibling sub-tasks and the terminal task. Record one — via the command below — at each of the following moments. Each note should be 1–2 sentences; before adding, re-read `senko contract note list <contract_id>` and skip the write if the same observation is already present.

For every note you add, wrap the write with the `contract_note_add` workflow-stage hooks. Emit pre-hooks, run `senko contract note add`, then emit post-hooks:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add pre
senko contract note add <contract_id> --content "<text>" --source-task <task_id>
bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add post
```

Execute any commands printed by the emit-hooks calls in order.

1. **Major design decisions**: as soon as a non-trivial technical choice is made (library or pattern selection, architectural change, non-obvious trade-off), write a note naming the decision and the reason. Do this during planning or implementation, whichever is earlier.
2. **Pitfalls / surprises**: when a non-obvious bug, undocumented constraint, or reproducible gotcha is hit, record it so the next sibling doesn't repeat the loss. One sentence of what went wrong + one sentence of what to do about it is enough.
3. **Task-completion summary**: just before running `senko task complete <id>` in the Finalization section, add a short summary note — what was done, what is explicitly left for other sub-tasks or the terminal, and any cross-cutting invariants newly established.
