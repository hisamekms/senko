# Contract-Terminal Task

A task with the `contract-terminal` tag exists solely to verify the linked Contract's Definition of Done at the end of a split. It does NOT go through the normal plan-and-implement cycle; its work is verification plus, on failure, spawning follow-up tasks.

This workflow supersedes both `execute-task.md` and `complete-task.md` for terminal tasks. You arrive here when either file detects the `contract-terminal` tag.

## Step 1: Pre-check

```bash
senko task get <id>
```

- Verify `status` is `todo` or `in_progress`. If something else, inform the user and stop.
- Verify `contract_id` is set (non-null). If it is null, this is a mis-tagged task — stop and report; `contract-terminal` without a contract is a bug, not an expected state.
- Load Contract state:
  ```bash
  senko contract get <contract_id>

  # Walk every page of contract notes — `contract note list` is cursor-paginated.
  CURSOR=""
  while :; do
    if [ -z "$CURSOR" ]; then
      PAGE=$(senko contract note list <contract_id> --limit 50)
    else
      PAGE=$(senko contract note list <contract_id> --limit 50 --after "$CURSOR")
    fi
    echo "$PAGE" | jq '.items[]'
    CURSOR=$(echo "$PAGE" | jq -r '.next_cursor // empty')
    [ -z "$CURSOR" ] && break
  done
  ```
  Hold the contract DoD, description, and notes in working context.
- Enumerate sibling tasks linked to the same Contract and verify they are all `completed`. Use the task's `dependencies` array (set up by `add-task.md` Phase 3) — every ID there should be a completed sub-task. For each dependency ID:
  ```bash
  senko task get <dep_id>
  ```
  If any dependency is not `completed`, stop and tell the user to finish those first.

## Step 2: Start

If the task is still `todo`, transition it (metadata handling follows `execute-task.md`'s Build-metadata step):

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_start
senko task start <id> --metadata '<final-metadata-json>'   # omit --metadata if empty
```

Terminal tasks normally have no `branch` set (there's no code change). Skip worktree creation. If a branch IS set, treat this as an exceptional case (perhaps the user wants to commit a follow-up doc or snapshot) and follow the normal `/wth` flow.

## Step 3: Verify Contract DoD

For each Contract DoD item with `"checked": false`:

1. Launch the `dod-verifier` subagent (via the Agent tool) with:
   - the Contract DoD text for that index
   - the Contract's full note list (decisions, pitfalls, completion summaries from sibling tasks)
   - the `description`, `plan`, and `definition_of_done` of every linked sub-task (run `senko task get <sub_id>` for each)
   - the Contract's title and description for framing
2. Process the subagent's result for that item:
   - **VERIFIED**: wrap the check with `contract_dod_check` workflow-stage hooks:
     ```bash
     bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_dod_check pre
     senko contract dod check <contract_id> <index>
     bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_dod_check post
     ```
     Execute any commands printed by the emit-hooks calls in order.
   - **NEEDS_USER_APPROVAL**: confirm with the user via `AskUserQuestion`; if approved, run the same pre-hook / `dod check` / post-hook sequence above
   - **NOT_ACHIEVED**: leave the DoD unchecked and append the gap to an in-memory `gaps` list that includes the DoD index, text, and the subagent's explanation

Do the DoD items sequentially unless they're clearly independent (the note context may be useful across items).

## Step 4: Branch on result

### Case A — all Contract DoDs are now checked

The Contract is satisfied. Complete the terminal task itself:

1. Run the `dod-verifier` subagent for any unchecked DoD items on the **terminal task** (its own DoD, typically just `"Verify Contract DoD items"` seeded in `add-task.md`). Process results the same way (VERIFIED → `senko task dod check`, NEEDS_USER_APPROVAL → ask, NOT_ACHIEVED → address it).
2. Record a closing note on the Contract, wrapped with `contract_note_add` workflow-stage hooks:
   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add pre
   senko contract note add <contract_id> \
     --content "Terminal verification passed on task <id>. All Contract DoD items checked." \
     --source-task <id>
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add post
   ```
   Execute any commands printed by the emit-hooks calls in order.
3. Consult `senko config` for `merge_via` and perform the PR-merge check exactly like `complete-task.md` does. Since terminal tasks usually have no branch, PR checks are rarely relevant — but if the user added a branch, respect the config.

4. Complete:
   ```bash
   senko task complete <id>
   ```

   Remind the user to clean up any worktree with `/wth rm`.

### Case B — one or more Contract DoDs remain unchecked (gaps)

The Contract is not satisfied. Create follow-up tasks linked to the same Contract, spawn a new terminal, and cancel the current terminal.

1. **Propose follow-up tasks** (usually one per gap, but merge closely related gaps into a single task if that keeps the work coherent):
   - Draft `title`, `description`, and `definition_of_done` for each, derived from the gap text + subagent rationale.
   - Confirm each follow-up with the user via `AskUserQuestion` before creating it. Allow the user to amend or drop any proposal.
2. **Create each follow-up task** (reuse `add-task.md` Phase 4 wiring: title, description, priority, tags, DoD, branch, `publish`):
   ```bash
   senko task add --title "<title>" --assignee-user-id self
   senko task edit <new_id> --contract <contract_id> --description "<text>" \
     --add-definition-of-done "<dod 1>"   # repeat for each DoD
   # ...branch setting per add-task.md Phase 4 step 4...
   senko task publish <new_id>
   ```
3. **Create a new terminal task** that depends on the new follow-ups:
   ```bash
   senko task add --title "Verify contract: <contract title> (retry)" --assignee-user-id self
   senko task edit <new_term_id> --contract <contract_id> --add-tag contract-terminal \
     --add-definition-of-done "Verify Contract DoD items"
   senko task deps set <new_term_id> --on <follow_up_1> <follow_up_2>
   senko task publish <new_term_id>
   ```
4. **Record a Contract note** explaining the gap and the retry plan (one note is enough), wrapped with `contract_note_add` workflow-stage hooks:
   ```bash
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add pre
   senko contract note add <contract_id> \
     --content "Terminal <id> found gaps on DoD #<i>, #<j>: <short reason>. Follow-ups <fu1>, <fu2>; new terminal <new_term_id>." \
     --source-task <id>
   bash ${CLAUDE_SKILL_DIR}/scripts/emit-hooks.sh contract_note_add post
   ```
   Execute any commands printed by the emit-hooks calls in order.
5. **Cancel the current terminal task** — it has fulfilled its purpose (discovering the gap) and a fresh one is in place:
   ```bash
   senko task cancel <id> --reason "Contract DoDs not met; follow-ups <fu1>, <fu2>; new terminal <new_term_id>"
   ```

Display the new task graph to the user so they can pick up where the terminal left off.

## Step 5: Post-completion (Case A only)

Remind the user to clean up the worktree (if any was created) via `/wth rm`. No post-completion action is required for Case B — the new terminal task handles the next iteration.
