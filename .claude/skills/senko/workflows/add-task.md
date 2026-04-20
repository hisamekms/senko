# Add Task

## Normal vs Simple Mode

- **Normal** (`add <description>`): Phase 1 → 2 → 3 → 4 (full workflow)
- **Simple** (`add --simple <description>`): Create draft → set description → `task publish` (no planning)

## Procedure

### Phase 1: Planning & Split Decision

> **Skip this phase in simple mode.**

Investigate the task through codebase exploration and conversation. Repeat until no open questions remain:

1. Based on the task description and codebase investigation, list **unclear points and decisions needed**
2. If the list is empty, proceed to split decision
3. For each item, ask the user via `AskUserQuestion`:
   - Present options for each question
   - Mark at least one option with "(Recommended)" when possible
4. After all questions are resolved, return to step 1 — previous answers may raise new questions

Once all questions are resolved, **decide whether to split the task**. Consider these heuristics:

- **Split** when:
  - The task contains independently workable parts (can be done in parallel by separate sessions)
  - Changes span multiple modules or layers with no code-level coupling
  - The task mixes distinct concerns (e.g., auto-fixable lints vs. manual refactoring)
  - Subtasks have different risk levels or review requirements
- **Keep as one** when:
  - All changes are tightly coupled and must be committed together
  - The task is small enough that splitting adds overhead without benefit
  - Splitting would create tasks that are trivial on their own

If splitting, define the sub-tasks with their titles and relationships. Ask the user via `AskUserQuestion` to confirm the proposed split.

#### Phase 1.5: Contract draft (split path only)

> Skip this sub-phase when keeping the task as a single task. Contracts are enforced **only** when the task is split.

Splitting requires a Contract — a shared aggregate that carries the Definition of Done the sub-tasks collectively satisfy, and that a terminal task verifies at the end. Prepare the Contract draft now (do not create it yet; Phase 2 issues the `senko contract add` call with the other writes).

1. **Derive a draft from the original task**:
   - `contract_title`: the original task title (or a slightly generalized phrasing)
   - `contract_description`: a summary of the combined goal that all sub-tasks serve
   - `contract_definition_of_done`: the DoD items the **whole split** must satisfy — things that are cross-cutting and can only be verified across sub-tasks (e.g. end-to-end behavior, integration tests, removed dead code, consistent API surface). Per-sub-task DoD stays on the individual sub-tasks.
   - `contract_tags`: optional; useful for grouping contracts of the same feature or initiative.
2. **Confirm with the user via `AskUserQuestion`** — ask whether the derived title, description, and DoD items are acceptable. Let the user accept, amend, or reject any field. Loop until the user is satisfied.
3. Record the confirmed values in local state for Phase 2.

### Phase 2: Draft Creation

#### Build add metadata

Run the metadata builder script to read `[workflow.task_add].metadata_fields` from config:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_add
```

Parse the JSON output (`{"resolved": {...}, "prompts": [...]}`):

- If `prompts` array is non-empty, ask the user each prompt question using `AskUserQuestion`. Merge user answers into `resolved`.
- If `resolved` is non-empty (has keys), pass `--metadata '<json>'` to each `senko task add` call below.

#### Create draft tasks

Create one or multiple draft tasks based on Phase 1 results.

**Single task:**

```bash
senko task add --title "<title>" --assignee-user-id self --metadata '<metadata-json>'
```

**Multiple tasks (split):**

The split path has a strict ordering — Contract must exist before the sub-tasks and terminal task can link to it.

1. **Create the Contract first** (using the draft confirmed in Phase 1.5):

   Emit pre-hooks for the `contract_add` workflow stage and execute any commands listed:

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_add pre
   ```

   Then create the contract:

   ```bash
   senko contract add \
     --title "<contract_title>" \
     --description "<contract_description>" \
     --definition-of-done "<dod 1>" \
     --definition-of-done "<dod 2>"
     # ... --tag for each contract_tag
   ```

   Capture the `id` from the JSON output — refer to it as `$CONTRACT_ID` below.

   Emit post-hooks for the `contract_add` workflow stage and execute any commands listed:

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_add post
   ```

2. **Create each sub-task**:

   ```bash
   senko task add --title "<sub-task 1 title>" --assignee-user-id self --metadata '<metadata-json>'
   senko task add --title "<sub-task 2 title>" --assignee-user-id self --metadata '<metadata-json>'
   # ...
   ```

   Capture each `id` — refer to them as `$SUB_ID_1`, `$SUB_ID_2`, …

3. **Auto-create the terminal task** — its sole job is to verify `$CONTRACT_ID` at the end:

   ```bash
   senko task add --title "Verify contract: <contract_title>" --assignee-user-id self
   ```

   Capture the `id` as `$TERMINAL_ID`.

4. **Link every task (sub-tasks + terminal) to the Contract**:

   ```bash
   senko task edit $SUB_ID_1 --contract $CONTRACT_ID
   senko task edit $SUB_ID_2 --contract $CONTRACT_ID
   # ...
   senko task edit $TERMINAL_ID --contract $CONTRACT_ID --add-tag contract-terminal \
     --add-definition-of-done "Verify Contract DoD items"
   ```

   The `contract-terminal` tag is what lets the skill route the terminal task to the Contract-verification workflow at execute/complete time. Do NOT omit it.

5. **Record a Contract note** summarizing the split (this seeds the shared memory for the sub-tasks):

   Emit pre-hooks for the `contract_note_add` workflow stage and execute any commands listed:

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add pre
   ```

   Add the note:

   ```bash
   senko contract note add $CONTRACT_ID \
     --content "Contract created at task split. Sub-tasks: $SUB_ID_1, $SUB_ID_2, ...; terminal: $TERMINAL_ID."
   ```

   Emit post-hooks for the `contract_note_add` workflow stage and execute any commands listed:

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add post
   ```

Omit `--metadata` entirely if there are no metadata fields to pass.

**Always include `--assignee-user-id self`** on every `senko task add` call above (both the single-task and split paths — including the terminal task). Unlike `--metadata`, this flag is never optional: omitting it leaves the task unassigned. Do NOT drop it.

Capture all the IDs for Phase 3.

### Phase 3: Dependency Setup

Set up dependencies between tasks:

1. Check existing active tasks for potential dependencies:

```bash
senko task list --status todo --status in_progress
```

2. For **split tasks**: set dependencies between the new tasks where one must complete before another can start (sequential relationships). Tasks that can run in parallel should have no dependency between them.

3. For **split tasks**: the terminal task (`$TERMINAL_ID`) must depend on **every** sub-task so it only becomes ready once all sub-tasks are completed:

   ```bash
   senko task deps set $TERMINAL_ID --on $SUB_ID_1 $SUB_ID_2  # ...and every other sub-task ID
   ```

4. For **all new tasks**: check if any depend on existing active tasks.

```bash
senko task deps add <task_id> --on <dep_id>
```

### Phase 4: Finalize Tasks

For each created task:

1. Update with planning results
2. Use `AskUserQuestion` to confirm:
   - Title and content are appropriate
   - Dependencies are correct
   - Tags to set
   - Priority (default p2) adjustment
3. Apply confirmed settings:

```bash
senko task edit <id> \
  --title "Final title" \
  --description "Planning description" \
  --priority p1 \
  --add-tag backend \
  --add-definition-of-done "Write unit tests" \
  --add-definition-of-done "E2E tests pass"
```

4. **Branch setting** (before `senko task publish`):
   - Determine whether the task involves repository operations (code changes, file edits, configuration changes, etc.) based on the task's title and description. If unclear, use `AskUserQuestion` to ask the user.
   - If the task does NOT involve repository operations (e.g., investigation only, external service setup), skip branch setting.
   - If the task involves repository operations:
     1. Read `branch_template` from `senko config`. If not set, use `{{id}}-{{slug}}` as the default template.
     2. Resolve template variables:
        - `{{id}}` → task ID
        - `{{slug}}` → kebab-case slug derived from the task title
        - `{{context.<key>}}` → resolve from session context. If unavailable, use `AskUserQuestion` to ask the user for the value.
        - `{{<name>:<opt1>|<opt2>|...}}` → enum variable. Infer the appropriate value from the task's title and description (e.g., new feature → `feat`, bug fix → `fix`, maintenance → `chore`). If unclear, use `AskUserQuestion` to present the options.
     3. Set it: `senko task edit <id> --branch <branch-name>`

5. Transition to todo:

```bash
senko task publish <id>
```

**Note on the terminal task**: its `--add-definition-of-done "Verify Contract DoD items"` (set in Phase 2 step 4) is usually the only DoD it needs. The user may add more in Phase 4 if the split has side-artifacts that should be verified at the terminal step. Its branch can be set with the normal `branch_template` flow — no special handling.

Display the finalized task details (or task graph if multiple) to the user. For split paths, also print `$CONTRACT_ID` so the user can reference it in subsequent sessions.

---

**Simple mode procedure:**

1. Create draft: `senko task add --title "<description>" --assignee-user-id self`
2. Set description: `senko task edit <id> --description "<description>"`
3. **Branch setting**: Same as Phase 4 step 4 above.
4. Transition: `senko task publish <id>`
