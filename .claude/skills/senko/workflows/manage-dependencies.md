# Manage Dependencies

Handle dependency operations based on the subcommand.

## `deps add <task_id> --on <dep_id>`

Add a dependency. senko will reject circular and self-dependencies automatically.

```bash
senko task deps add <task_id> --on <dep_id>
```

## `deps remove <task_id> --on <dep_id>`

Remove a dependency.

```bash
senko task deps remove <task_id> --on <dep_id>
```

## `deps list <task_id>`

Show all tasks that the given task depends on. Walk every page — `deps list` is cursor-paginated.

```bash
CURSOR=""
while :; do
  if [ -z "$CURSOR" ]; then
    PAGE=$(senko task deps list <task_id> --limit 50)
  else
    PAGE=$(senko task deps list <task_id> --limit 50 --after "$CURSOR")
  fi
  echo "$PAGE" | jq '.items[]'
  CURSOR=$(echo "$PAGE" | jq -r '.next_cursor // empty')
  [ -z "$CURSOR" ] && break
done
```

Display results to the user. If there are unresolved dependencies, note which ones are blocking.
