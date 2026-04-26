# Add Task

## Normal vs Simple Mode

- **Normal** (`add <description>`): Phase 0 → 1 → 2 → 3 → 4 (full workflow)
- **Simple** (`add --simple <description>`): Create draft → set description → `task publish` (no planning)

## Procedure

### Phase 0: Initialize narrative state

> **Skip this phase in simple mode.**

The pre-publication review at Phase 4 step 5 reads its input from two files
(`narrative.md` and `packet.md`) addressed by a short `$NID`. Initialize that state
**before** Phase 1 so every decision and constraint can be appended in real time.

Capture the original user intent verbatim — the literal request as it appeared in the
chat — then run:

```bash
NID=$(echo '{"intent":"<original user intent>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh init)
```

`$NID` is an 8-char alphanumeric ID. Carry it through every later phase. Keep it in
session memory; do **not** rebuild it.

### Phase 1: Planning & Split Decision

> **Skip this phase in simple mode.**

Investigate the task through codebase exploration and conversation. Repeat until no open questions remain:

1. Based on the task description and codebase investigation, list **unclear points and decisions needed**
2. If the list is empty, proceed to split decision
3. For each item, ask the user via `AskUserQuestion`:
   - Present options for each question
   - Mark at least one option with "(Recommended)" when possible
   - **As soon as the user answers, record the decision** in the narrative:

     ```bash
     echo '{"q":"<question>","a":"<chosen answer>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-decision $NID
     ```

     For decisions made without an explicit AskUserQuestion (e.g., a judgment call you
     announced and the user accepted), use the `note` form instead:

     ```bash
     echo '{"note":"<decision text>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-decision $NID
     ```
4. After all questions are resolved, return to step 1 — previous answers may raise new questions

If the conversation surfaces a **cross-cutting constraint** (merge freeze window, fixed
interface, environment requirement, locked-in library/version, etc.) record it
immediately:

```bash
echo '{"text":"<constraint>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-constraint $NID
```

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

If splitting, define the sub-tasks with their titles and relationships. Ask the user via `AskUserQuestion` to confirm the proposed split. Record the split rationale (or the rationale for keeping single) via `append-decision` once the user confirms.

#### Phase 1.5: Contract draft (split path only)

> Skip this sub-phase when keeping the task as a single task. Contracts are enforced **only** when the task is split.

Splitting requires a Contract — a shared aggregate that carries the Definition of Done the sub-tasks collectively satisfy, and that a terminal task verifies at the end. Prepare the Contract draft now (do not create it yet; Phase 2 issues the `senko contract add` call with the other writes).

1. **Derive a draft from the original task**:
   - `contract_title`: the original task title (or a slightly generalized phrasing)
   - `contract_description`: a summary of the combined goal that all sub-tasks serve
   - `contract_definition_of_done`: the DoD items the **whole split** must satisfy — things that are cross-cutting and can only be verified across sub-tasks (e.g. end-to-end behavior, integration tests, removed dead code, consistent API surface). Per-sub-task DoD stays on the individual sub-tasks.
   - `contract_tags`: optional; useful for grouping contracts of the same feature or initiative.
2. **Confirm with the user via `AskUserQuestion`** — ask whether the derived title, description, and DoD items are acceptable. Let the user accept, amend, or reject any field. Loop until the user is satisfied. Record each confirmed field via `append-decision $NID` (use the `note` form if the question is multi-part).
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
     --add-definition-of-done "Reconcile Contract DoD: verify met items; spawn follow-up tasks for any unmet items"
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

1. Check existing active tasks for potential dependencies (walk every page — `task list` is cursor-paginated):

```bash
CURSOR=""
while :; do
  if [ -z "$CURSOR" ]; then
    PAGE=$(senko task list --status todo --status in_progress --limit 50)
  else
    PAGE=$(senko task list --status todo --status in_progress --limit 50 --after "$CURSOR")
  fi
  echo "$PAGE" | jq '.items[]'
  CURSOR=$(echo "$PAGE" | jq -r '.next_cursor // empty')
  [ -z "$CURSOR" ] && break
done
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

   Append each confirmed answer via `append-decision $NID` so the reviewer can see the
   reasoning.
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

5. **Pre-publication review (mandatory)**:

   Before `senko task publish`, every draft task must pass a reviewer agent. The agent depends on the path:

   - **Split path (Contract exists)** → use `task-contract-reviewer`. Reviews the Contract together with every linked sub-task and the terminal task.
   - **Single-task path (no Contract)** → use `single-task-reviewer`. Reviews the single draft task in isolation.

   The reviewer reads the **narrative** and **packet** files written by
   `senko-narrative.sh` rather than receiving JSON inline in the prompt. **Always pass
   both paths.** Never paste task/contract JSON into the agent prompt.

   ##### 5a. Split path

   1. Build the packet from current state. Always pass `--mode`, `--contract`, and the
      full list of `--tasks` (sub-tasks plus the terminal task) — `build-packet` does
      NOT remember args between calls.

      ```bash
      PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
        --mode split \
        --contract $CONTRACT_ID \
        --tasks $SUB_ID_1 $SUB_ID_2 ... $TERMINAL_ID)
      NARRATIVE_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh path $NID)
      ```

   2. Invoke the `task-contract-reviewer` agent with the two paths in the prompt:

      ```
      Narrative path: <NARRATIVE_PATH>
      Review Packet path: <PACKET_PATH>
      ```

      Do **not** include any other content. The agent reads both files via the Read tool.

   3. Read the agent's verdict:

      - **PASS** → proceed to step 6 (publish).
      - **PASS_WITH_MINOR_FIXES** → present the minor fixes to the user via `AskUserQuestion`. Apply the accepted fixes using `senko task edit`, `senko contract edit`, or `senko contract note add` as appropriate. Append each accepted fix via `append-decision $NID`. Then proceed to step 6.
      - **BLOCKING_FIXES_REQUIRED** → apply every blocking fix before publication. Apply Contract additions via `senko contract edit --description …` or `--add-definition-of-done …`. Append decision-log entries via `senko contract note add`. Apply task-specific additions via `senko task edit <id> --background … / --description … / --plan-file … / --in-scope … / --out-of-scope … / --add-definition-of-done … / --metadata …`. Apply dependency fixes via `senko task deps add/remove/set`. Then **rebuild the packet with the FULL args** (mode/contract/tasks — NOT just the changed task IDs) and **re-invoke the reviewer**:

        ```bash
        PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
          --mode split --contract $CONTRACT_ID \
          --tasks $SUB_ID_1 $SUB_ID_2 ... $TERMINAL_ID)
        ```

        Loop until the verdict is PASS or PASS_WITH_MINOR_FIXES. Append each fix as a
        decision via `append-decision $NID` so subsequent reviewer turns see the
        reasoning.
      - **INSUFFICIENT_PACKET** → the reviewer reports missing headings or unreadable files. Resolve the gap before retrying:
        - Missing `# Decisions` or `# Known constraints` heading in the narrative → re-run `senko-narrative.sh init` is **not** the fix (init creates a new ID). Instead, the heading should already be present from init; if it is missing, the narrative file has been corrupted manually — restore it.
        - Empty narrative section is fine; if the reviewer complains about empty sections, push back.
        - Missing `# Contract` or `# Tasks` heading in the packet → re-run `build-packet` with the full split args.
        - Wrong/stale paths → recompute via `senko-narrative.sh path $NID` / `packet-path $NID`.

        After resolving, rebuild the packet (full args) and re-invoke the reviewer.

   ##### 5b. Single-task path

   1. Build the single-mode packet:

      ```bash
      PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
        --mode single \
        --tasks $TASK_ID)
      NARRATIVE_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh path $NID)
      ```

   2. Invoke the `single-task-reviewer` agent with the two paths:

      ```
      Narrative path: <NARRATIVE_PATH>
      Review Packet path: <PACKET_PATH>
      ```

      The agent reads both files via the Read tool.

   3. Read the agent's verdict:

      - **PASS** → proceed to step 6 (publish).
      - **PASS_WITH_MINOR_FIXES** → present the minor fixes to the user via `AskUserQuestion`. Apply the accepted fixes using `senko task edit`. Append each accepted fix via `append-decision $NID`. Then proceed to step 6.
      - **BLOCKING_FIXES_REQUIRED** → apply every blocking fix via `senko task edit <id> --background … / --description … / --plan-file … / --in-scope … / --out-of-scope … / --add-definition-of-done … / --metadata … / --add-tag … / --priority …`. Apply dependency fixes via `senko task deps add/remove`. Then **rebuild the packet with the FULL args** (mode/tasks) and **re-invoke the reviewer**:

        ```bash
        PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
          --mode single --tasks $TASK_ID)
        ```

        Loop until the verdict is PASS or PASS_WITH_MINOR_FIXES. Append each fix as a
        decision via `append-decision $NID`.
      - **SHOULD_SPLIT_TASK** → the reviewer judged that the work should not remain a single task. Present the proposed split to the user via `AskUserQuestion`. If the user agrees:
        - Append a decision noting the restructure: `echo '{"note":"Initial single-task draft restructured into split after reviewer SHOULD_SPLIT_TASK; user confirmed."}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-decision $NID`.
        - **Keep the same `$NID`** — do NOT call `init` again. Re-enter Phase 1.5 (Contract draft) and Phase 2 split path with the proposed sub-tasks; the existing draft task may be reused as one of the sub-tasks (re-link with `--contract`) or canceled with `senko task cancel <id> --reason "Restructured into split"`.
        - When you reach step 5a, call `build-packet $NID --mode split --contract $NEW_CONTRACT_ID --tasks ...` to overwrite the packet with the new split metadata.

        If the user rejects the split, treat the verdict as PASS_WITH_MINOR_FIXES and continue.
      - **INSUFFICIENT_PACKET** → same handling as the split path: missing packet headings → rebuild with full args; missing narrative headings → restore the file.

   ##### 5c. Common

   Show the user a brief summary of the verdict and the changes applied (if any) before publishing.

6. Transition to todo:

```bash
senko task publish <id>
```

**Note on the terminal task**: its `--add-definition-of-done "Reconcile Contract DoD: verify met items; spawn follow-up tasks for any unmet items"` (set in Phase 2 step 4) is usually the only DoD it needs. The user may add more in Phase 4 if the split has side-artifacts that should be verified at the terminal step. Its branch can be set with the normal `branch_template` flow — no special handling.

Display the finalized task details (or task graph if multiple) to the user. For split paths, also print `$CONTRACT_ID` so the user can reference it in subsequent sessions.

---

**Simple mode procedure:**

1. Create draft: `senko task add --title "<description>" --assignee-user-id self`
2. Set description: `senko task edit <id> --description "<description>"`
3. **Branch setting**: Same as Phase 4 step 4 above.
4. Transition: `senko task publish <id>`
