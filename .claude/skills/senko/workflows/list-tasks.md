# List Tasks

Retrieve and display tasks using `senko task list`.

```bash
senko --output text task list
```

Use `--status`, `--tag`, and `--ready` to filter results. Combine filters as needed.

## Pagination

`task list` returns a page with at most `--limit` items (default 50, max 200) and an opaque `next_cursor`.

```json
{
  "items": [ ... ],
  "next_cursor": "eyJpZCI6MjB9"
}
```

- If `next_cursor` is `null`, there are no more results.
- Pass it back unchanged as `--after <cursor>` to fetch the next page.
- The cursor is opaque; do not decode or synthesize it.

Walk all pages (JSON output):

```bash
CURSOR=""
while :; do
  if [ -z "$CURSOR" ]; then
    PAGE=$(senko task list --limit 50)
  else
    PAGE=$(senko task list --limit 50 --after "$CURSOR")
  fi
  echo "$PAGE" | jq '.items[]'
  CURSOR=$(echo "$PAGE" | jq -r '.next_cursor // empty')
  [ -z "$CURSOR" ] && break
done
```

With `--output text`, the line `... more: --after <cursor>` is appended whenever `next_cursor` is set.
