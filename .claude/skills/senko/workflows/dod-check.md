# DoD Check/Uncheck

Manage the checked state of Definition of Done items. Indices are **1-based** (first item = 1).

## `dod check <task_id> <index> [--note "..."]`

Mark a DoD item as done. When the item required actual execution
(`verification_type: execution`), ALWAYS record how it was verified with `--note`
(the command you ran and its result):

```bash
senko task dod check <task_id> <index> --note "mise run e2e — 42/42 passed"
```

`--note` is optional for `static`/`manual` items but recommended whenever there
is concrete evidence worth recording.

## `dod uncheck <task_id> <index>`

Unmark a DoD item. This also clears any recorded verification note, since the
note documents the check being undone:

```bash
senko task dod uncheck <task_id> <index>
```

## Verification types

Every DoD item declares how it must be verified before it may be checked:

| Type | Meaning | How to verify |
| --- | --- | --- |
| `static` | Verifiable by inspection | Check files/code/artifacts exist and match |
| `execution` | Must actually run | Run the declared `verification_method` (tests, commands, the app) — static inspection is NOT sufficient |
| `manual` | Human judgment | Ask the user for approval |
| `unspecified` | Legacy item (pre-migration) | Judge from the item text; err toward stricter. Cannot be set on new items |

## Display format

DoD items show their check state, verification type, declared method, and recorded note:

- **Text output**:

  ```
  [x] E2E tests pass [execution]
        verify: run `mise run e2e` and confirm all pass
        verified: mise run e2e — 42/42 passed
  [ ] README has a usage section [static]
  ```

- **JSON output**: `{"content": "E2E tests pass", "checked": true, "verification_type": "execution", "verification_method": "run `mise run e2e` and confirm all pass", "verification_note": "mise run e2e — 42/42 passed"}`
