# Development

[日本語](DEVELOPMENT.ja.md)

## Status Transitions

```
draft → todo → in_progress → completed
                    ↓
                 canceled
```

- `draft` → `todo` → `in_progress` → `completed`: forward-only
- Any active state → `canceled`: always allowed
- Backward transitions and self-transitions are rejected

## Data Storage

The database is stored at `<project_root>/.localflow/data.db` (auto-created).

Project root is detected by searching for `.localflow/`, `.git/`, or using the current directory.

## Testing

```bash
cargo test                    # Unit tests
bash tests/e2e/run.sh         # E2E tests
```
