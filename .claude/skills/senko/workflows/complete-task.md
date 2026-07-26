# Complete Task

> **Terminal-task redirect**: if the task carries the `contract-terminal` tag, do NOT follow this file. Use `${CLAUDE_SKILL_DIR}/workflows/contract-terminal.md` — terminal completion is integrated with Contract DoD verification (including the create-follow-ups path on failure).

Mark a task as completed. `complete` will fail if any DoD items are unchecked.

```bash
senko task get <id>
```

1. Verify the task is in `in_progress` status. If not, inform the user and stop.
2. Check if any DoD items are unchecked (`"checked": false` in JSON, or `[ ]` in text output). If unchecked items exist:
   - Launch the `dod-verifier` agent (via Agent tool) with the task ID and unchecked DoD items — include each item's `verification_type` and `verification_method` so the agent knows whether it must actually run something (`execution`) or may verify by inspection (`static`)
   - Process the subagent's results for each item:
     - **VERIFIED**: `senko task dod check <id> <index> --note "<how it was verified>"` — pass the agent's Note (for `execution` items: the exact command run and its result) so the audit trail shows the item was actually executed, not just statically inspected
     - **NEEDS_USER_APPROVAL**: Use `AskUserQuestion` to confirm with the user, then check if approved (`--note "approved by user"`)
     - **NOT_ACHIEVED**: Inform the user that the item is not yet achieved
   - All DoD items must be checked before proceeding to complete
3. Check the workflow configuration (`senko config`):
   - If `merge_via = "pr"`:
     - Ensure `pr_url` is set on the task (`senko task edit <id> --pr-url <url>`)
     - Begin PR polling loop:
       1. Run `gh pr view <pr_url> --json state,reviews,comments`
       2. If there are new review comments or requested changes:
          - Address each review comment (fix code, respond to feedback)
          - Push the changes and continue polling
       3. If the PR state is MERGED:
          - Exit the polling loop and proceed to `senko task complete <id>`
       4. Otherwise, wait 1 minute and repeat from step 1
     - Use `--skip-pr-check` to bypass the merged check if needed
   - If `merge_via = "direct"` (default): no PR checks are performed

### Build completion metadata

Run the metadata builder script to read `[workflow.task_complete].metadata_fields` from config:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/build-metadata.sh task_complete
```

Parse the JSON output (`{"resolved": {...}, "prompts": [...]}`):

- If `prompts` array is non-empty, ask the user each prompt question using `AskUserQuestion`. Merge user answers into `resolved`.
- If `resolved` is non-empty (has keys):
  1. Get the task's current metadata from `senko task get <id>` (the `metadata` field in JSON output, or `null` if unset)
  2. Shallow-merge the new fields into the existing metadata object (new keys override existing keys)
  3. Save the merged metadata: `senko task edit <id> --metadata '<merged-json>'`

```bash
senko task complete <id>
```

Display the completed task info to the user. If there is an associated worktree, remind the user to clean it up.
