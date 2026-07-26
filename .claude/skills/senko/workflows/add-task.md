# Add Task

## Modes

- **Normal** (`add <description>`): Phase 0 → 1 → 2 → 3 → 4 (full workflow with reviewer + user approval).
- **Simple** (`add --simple <description>`): Create draft → set description → branch (if needed) → publish. No planning, no reviewer, no extra approval.

## CLI flag conventions

This skill writes content to draft tasks via `senko task edit`. On `edit`, **list fields** require **prefixed** flags:

- `--add-in-scope`, `--add-out-of-scope`, `--add-definition-of-done`, `--add-tag` to append.
- `--remove-...` / `--set-...` to remove or replace.
- Plain `--in-scope`, `--out-of-scope`, `--definition-of-done`, `--tag` do **not** exist on `edit` and will fail. (They exist only on `senko task add`.)

The `plan` field must **never** be set in this workflow. It is populated by the executor agent during implementation, not at registration time. Do not pass `--plan` or `--plan-file` to any command — including when applying reviewer-recommended fixes. If a reviewer (against its own guidelines) suggests plan content, ignore it.

## Procedure

### Phase 0: Initialize narrative state

> Skip in simple mode.

The pre-publication review at Phase 4 reads its input from two files (`narrative.md`, `packet.md`) addressed by a short `$NID`. Initialize this state **before** Phase 1 so every decision and constraint can be appended in real time.

Capture the original user intent verbatim — the literal request as it appeared in the chat — then run:

```bash
NID=$(echo '{"intent":"<original user intent>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh init)
```

`$NID` is an 8-char alphanumeric ID. Carry it through every later phase.

### Phase 1: Planning & Split Decision

> Skip in simple mode.

Investigate the task through codebase exploration and conversation. Repeat until no open questions remain:

1. List **unclear points and decisions needed** based on the task description and codebase investigation.
2. If empty, proceed to the split decision.
3. For each item, ask the user via `AskUserQuestion` (mark at least one option "(Recommended)" when possible). **As soon as the user answers, record the decision** in the narrative:

   ```bash
   echo '{"q":"<question>","a":"<chosen answer>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-decision $NID
   ```

   For decisions made without an explicit `AskUserQuestion` (a judgment call you announced and the user accepted), use the `note` form:

   ```bash
   echo '{"note":"<decision text>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-decision $NID
   ```
4. Loop back to step 1 — earlier answers may raise new questions.

If the conversation surfaces a **cross-cutting constraint** (merge freeze, fixed interface, environment requirement, locked-in library/version, etc.) record it immediately:

```bash
echo '{"text":"<constraint>"}' | bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh append-constraint $NID
```

Once all questions are resolved, decide whether to split:

- **Split** when subtasks are independently workable, span loosely-coupled modules/layers, mix distinct concerns, or have differing risk/review levels.
- **Keep as one** when changes are tightly coupled, the task is small, or splitting would create trivial subtasks.

If splitting, define the sub-tasks and their relationships. Confirm via `AskUserQuestion`. Record the rationale (split or single) via `append-decision`.

#### Phase 1.5: Contract draft (split path only)

> Skip when keeping as a single task. Contracts are enforced **only** when the task is split.

Splitting requires a Contract — a shared aggregate carrying the cross-cutting Definition of Done that a terminal task verifies at the end. Prepare the Contract draft now (do not create it yet; Phase 2 issues `senko contract add` together with the other writes).

1. Draft from the original task:
   - `contract_title`: the original task title (or slightly generalized).
   - `contract_description`: a summary of the combined goal that all sub-tasks serve.
   - `contract_definition_of_done`: cross-cutting DoD verifiable only across sub-tasks (end-to-end behavior, integration tests, removed dead code, consistent API surface). Per-sub-task DoD stays on the individual sub-tasks.
   - `contract_tags` (optional): useful for grouping.
2. Confirm with the user via `AskUserQuestion` — accept, amend, or reject any field. Loop until satisfied. Record each confirmed field via `append-decision $NID` (use the `note` form for multi-part questions).

### Phase 2: Draft Creation

#### Build add metadata

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_add
```

Parse the JSON output (`{"resolved": {...}, "prompts": [...]}`):

- If `prompts` is non-empty, ask each via `AskUserQuestion` and merge the answers into `resolved`.
- If `resolved` has any keys, pass `--metadata '<json>'` to each `senko task add` below. Otherwise omit `--metadata`.

#### Create draft tasks

**Always include `--assignee-user-id self`** on every `senko task add` call (single-task and split paths, including the terminal task). Omitting it leaves the task unassigned.

**Single task:**

```bash
senko task add --title "<title>" --assignee-user-id self --metadata '<metadata-json>'
```

Capture the `id` as `$TASK_ID`.

**Split path** (strict ordering — Contract must exist before sub-tasks and the terminal can link to it):

1. Pre/create/post the Contract:

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_add pre

   senko contract add \
     --title "<contract_title>" \
     --description "<contract_description>" \
     --definition-of-done "[execution] <dod 1> :: <how to verify>" \
     --definition-of-done "[static] <dod 2>"
     # ... --tag for each contract_tag
     # DoD format: "[static|execution|manual] <content>[ :: <verification method>]" — see "Writing DoD items" below

   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_add post
   ```

   Capture the `id` as `$CONTRACT_ID`.

2. Create each sub-task:

   ```bash
   senko task add --title "<sub-task title>" --assignee-user-id self --metadata '<metadata-json>'
   ```

   Capture each `id` as `$SUB_ID_1`, `$SUB_ID_2`, …

3. Auto-create the terminal task — its sole job is to verify `$CONTRACT_ID` at the end:

   ```bash
   senko task add --title "Verify contract: <contract_title>" --assignee-user-id self
   ```

   Capture the `id` as `$TERMINAL_ID`.

4. Link every task to the Contract (and tag the terminal):

   ```bash
   senko task edit $SUB_ID_1 --contract $CONTRACT_ID
   senko task edit $SUB_ID_2 --contract $CONTRACT_ID
   # ...
   senko task edit $TERMINAL_ID --contract $CONTRACT_ID --add-tag contract-terminal \
     --add-definition-of-done "[manual] Reconcile Contract DoD: verify met items; spawn follow-up tasks for any unmet items"
   ```

   The `contract-terminal` tag routes the terminal task to the Contract-verification workflow at execute/complete time. Do NOT omit it.

5. Record a Contract note summarizing the split (seeds shared memory for sub-tasks):

   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add pre

   senko contract note add $CONTRACT_ID \
     --content "Contract created at task split. Sub-tasks: $SUB_ID_1, $SUB_ID_2, ...; terminal: $TERMINAL_ID."

   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add post
   ```

### Phase 3: Dependency Setup

1. Walk every page of existing active tasks (cursor-paginated) to spot potential dependencies:

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

2. **Split path**: set sequential dependencies between sub-tasks where ordering matters. Sub-tasks that can run in parallel have no edge between them.

3. **Split path**: the terminal task must depend on **every** sub-task so it only becomes ready once all sub-tasks complete:

   ```bash
   senko task deps set $TERMINAL_ID --on $SUB_ID_1 $SUB_ID_2  # ... and every other sub-task ID
   ```

4. **All paths**: link to existing active tasks where required:

   ```bash
   senko task deps add <task_id> --on <dep_id>
   ```

### Phase 4: Finalize Tasks

#### 4.1 Apply planning results

For each draft task, apply the planning results from Phase 1 (and any Contract refinements for the split path) using `senko task edit`. Use `--add-...` for list fields (see "CLI flag conventions" above).

```bash
senko task edit <id> \
  --title "<final title>" \
  --background "<background>" \
  --description "<description>" \
  --priority p1 \
  --add-tag <tag> \
  --add-definition-of-done "[<type>] <dod>[ :: <verification method>]" \
  --add-in-scope "<scope>" \
  --add-out-of-scope "<non-goal>"
```

For the split path, also refine the Contract if needed:

```bash
senko contract edit $CONTRACT_ID \
  --description "<description>" \
  --add-definition-of-done "[<type>] <dod>[ :: <verification method>]" \
  --add-tag <tag>
```

Do not pass `--plan` / `--plan-file`.

**Writing DoD items.** Every DoD item must declare a verification type; plain untagged strings are rejected:

- `[static]` — verifiable by inspecting code/artifacts (file exists, section added, dead code removed)
- `[execution]` — must actually be run to verify (tests pass, command succeeds, app behavior). Whenever a concrete command exists, declare it after ` :: ` (e.g. `"[execution] E2E tests pass :: run mise run e2e, all green"`), so the verifier at completion time runs exactly that.
- `[manual]` — needs human judgment (UX quality, wording, approval)

Choose `[execution]` whenever the item's real intent is "it works", not "the code is there" — this prevents the completion-time verifier from checking off runtime behavior via static inspection alone.

#### 4.2 Branch setting

Determine whether the task involves repository operations (code, file, or config changes). If unclear, ask via `AskUserQuestion`. If it does NOT (investigation only, external service setup, etc.), skip branch setting.

If it does:

1. Read `branch_template` from `senko config`. Default: `{{id}}-{{slug}}`.
2. Resolve template variables:
   - `{{id}}` → task ID.
   - `{{slug}}` → kebab-case slug from the title.
   - `{{context.<key>}}` → from session context, or ask via `AskUserQuestion` if unavailable.
   - `{{<name>:<opt1>|<opt2>|...}}` → enum. Infer from title/description (new feature → `feat`, bug fix → `fix`, maintenance → `chore`). If unclear, ask via `AskUserQuestion`.
3. `senko task edit <id> --branch <branch-name>`

#### 4.3 Pre-publication review

Every draft task must pass a reviewer agent before publication. The reviewer reads the **narrative** and **packet** files written by `senko-narrative.sh`. **Always pass both paths.** Never paste task/contract JSON into the agent prompt.

- **Split path** (Contract exists) → `task-contract-reviewer`
- **Single-task path** (no Contract) → `single-task-reviewer`

##### 4.3a Build the packet and invoke the reviewer

Build the packet from current state. `build-packet` does NOT remember args between calls — always pass the FULL set.

**Split path:**

```bash
PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
  --mode split \
  --contract $CONTRACT_ID \
  --tasks $SUB_ID_1 $SUB_ID_2 ... $TERMINAL_ID)
NARRATIVE_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh path $NID)
```

**Single-task path:**

```bash
PACKET_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh build-packet $NID \
  --mode single \
  --tasks $TASK_ID)
NARRATIVE_PATH=$(bash ${CLAUDE_SKILL_DIR}/scripts/senko-narrative.sh path $NID)
```

Invoke the reviewer with **only** these two lines as the prompt (the agent reads both files via the Read tool):

```
Narrative path: <NARRATIVE_PATH>
Review Packet path: <PACKET_PATH>
```

##### 4.3b Handle the verdict

Reviewer fixes are **applied automatically** without asking the user. Append every applied fix via `append-decision $NID` so subsequent reviewer turns see the reasoning.

- **PASS** → proceed to 4.4.
- **PASS_WITH_MINOR_FIXES** → auto-apply every minor fix, append decisions, proceed to 4.4.
- **BLOCKING_FIXES_REQUIRED** → auto-apply every blocking fix, append decisions, then **rebuild the packet with the FULL args** and **re-invoke the reviewer**. Loop until PASS or PASS_WITH_MINOR_FIXES.
- **SHOULD_SPLIT_TASK** (single-task only) → present the proposed split to the user via `AskUserQuestion`. If accepted: append `{"note":"Initial single-task draft restructured into split after reviewer SHOULD_SPLIT_TASK; user confirmed."}`, **keep the same `$NID`** (do NOT call `init` again), then re-enter Phase 1.5 + Phase 2 split path. The existing draft can be reused as a sub-task (re-link with `--contract`) or canceled (`senko task cancel <id> --reason "Restructured into split"`). When you reach 4.3a, call `build-packet` with the new split args. If rejected, treat as PASS_WITH_MINOR_FIXES and continue.
- **INSUFFICIENT_PACKET** → resolve the gap: missing packet headings → rebuild with full args; missing narrative headings → restore the file (do NOT call `init` again — it would create a new ID). Then re-invoke.

**Editing CLI cheat-sheet** for applying fixes:

| Target | Command |
|---|---|
| Task body | `senko task edit <id> --background … --description … --add-definition-of-done … --add-in-scope … --add-out-of-scope … --add-tag … --priority …` |
| Task metadata | `senko task edit <id> --metadata '<json>'` |
| Task deps | `senko task deps add/remove/set <id> --on <ids>` |
| Contract body | `senko contract edit $CONTRACT_ID --description … --add-definition-of-done …` |
| Contract decision-log | `senko contract note add $CONTRACT_ID --content "…"` |

Never use `--plan` / `--plan-file`. Use `--add-...` (not `--...`) for list fields on `edit`.

#### 4.4 Final user approval

Before publishing, present the finalized task(s) to the user as **human-readable Markdown** (not JSON) and wait for the user's free-text response.

Render each task with this structure:

```markdown
## #<id> — <title>

| | |
|---|---|
| Priority | <priority> |
| Tags | <tag>, <tag> |
| Branch | <branch or "(none)"> |
| Dependencies | <dep_id>, <dep_id> (or "none") |

### Background
<background>

### Description
<description>

### Definition of Done
- <item>
- <item>

### In Scope
- <item>

### Out of Scope
- <item>
```

For the split path, render the Contract first (title, description, DoD, tags), then each sub-task in dependency order, then the terminal task last.

After printing the rendered Markdown, ask the user via `AskUserQuestion` how to proceed. Use a single question with three options:

- `header`: "Publish?"
- `question`: "Publish the finalized task(s), modify them, or cancel?"
- options:
  - **Publish (Recommended)** — "Publish the task(s) as shown above and end this workflow."
  - **Modify** — "Make changes to the task(s) before publishing. You will be asked for the modification details in plain text after selecting this."
  - **Cancel** — "Stop the workflow. The draft task(s) and Contract (if any) remain as-is — re-run `senko task publish <id>` later to resume."

Adjust wording to the user's language; the option semantics must match the three branches below.

Handle the response:

- **Publish** → proceed to 4.5.
- **Modify** → in the next assistant message, ask the user for the modification details **in plain text** (free-form text gives the most expressive feedback channel — do not use `AskUserQuestion` for this follow-up). After the user replies, apply the changes via the 4.3b editing cheat-sheet, append `{"note":"User-requested change at final approval: <summary>"}` via `append-decision $NID`, then **loop back to 4.3a** (rebuild packet → reviewer → handle verdict → re-render → ask again). The reviewer ensures user edits don't reintroduce gaps.
- **Cancel** → leave every draft task and the Contract (if any) untouched (do **not** call `senko task cancel`). Tell the user the draft IDs are preserved and that they can resume later with `senko task publish <id>`. End the workflow.

#### 4.5 Publish

```bash
senko task publish <id>
```

The terminal task's DoD set in Phase 2 step 4 is usually the only DoD it needs; add more in 4.1 only if the split has side-artifacts to verify at the terminal step. Its branch follows the normal `branch_template` flow.

After publishing, print `$CONTRACT_ID` (split path) so the user can reference it later.

---

## Simple mode procedure

1. Create draft: `senko task add --title "<description>" --assignee-user-id self`
2. Set description: `senko task edit <id> --description "<description>"`
3. Branch: same as Phase 4.2.
4. Publish: `senko task publish <id>`
