# Config Overview

senko's configuration is TOML. Multiple files are merged, and only the **sections matching the active runtime** are activated.

## File Layout and Precedence

Resolution order (higher wins):

1. **CLI flags** (`--config <path>`, `--port`, `--host`, …)
2. **Environment variables** (`SENKO_*`)
3. **Local config** `.senko/config.local.toml` — not committed; per-developer overrides
4. **Project config** `.senko/config.toml` — committed; team-shared
5. **User config** `~/.config/senko/config.toml` — shared across all projects
6. **Built-in defaults**

When the same key appears at multiple layers, **scalars are overridden by the higher layer; tables (e.g. hooks) merge by name**.

Generate a template:

```bash
senko config --init > .senko/config.toml
```

## Top-Level Sections

| Section | Active when | Details |
|---|---|---|
| `[project]` / `[user]` / `[log]` | Always | [`[project]` / `[user]` / `[log]` Config](common.md) |
| `[backend.sqlite]` / `[backend.postgres]` | With a direct backend | [`[server.*]` / `[backend.*]` / `[server.auth.*]` Config](server-remote.md) |
| `[cli]` / `[cli.remote]` | Local CLI (anything other than `serve`) | [`[cli.*]` Config](cli.md) |
| `[server]` / `[server.auth.*]` / `[server.remote]` | `senko serve` direct mode | [`[server.*]` / `[backend.*]` / `[server.auth.*]` Config](server-remote.md) |
| `[server.relay]` | `senko serve` relay mode (activates when `url` is set) | [`[server.relay.*]` Config](server-relay.md) |
| `[workflow]` / `[workflow.<stage>]` | Consumed by the skill (may be read in any runtime) | [`[workflow.*]` Config](workflow.md) |

## Runtime Filtering

**Hooks under a section that doesn't match the active runtime do not fire.** For example, `[server.remote.*]` hooks are not read while `senko task add` (the `cli` runtime) is running.

When mismatched sections are found, a single warning is emitted at startup:

```
hooks configured under runtime sections that do not match the active runtime; they will not fire
```

See [Choosing a Runtime](../../explanation/runtimes.md) for picking a runtime.

## Handling Secrets

When using AWS Secrets Manager (requires the `aws-secrets` feature build):

| Direct value | ARN form |
|---|---|
| `SENKO_AUTH_API_KEY_MASTER_KEY` | `SENKO_AUTH_API_KEY_MASTER_KEY_ARN` |
| `[server.auth.api_key] master_key` | `[server.auth.api_key] master_key_arn` |
| `[backend.postgres] url` | `[backend.postgres] url_arn` or `rds_secrets_arn` |

ARN-specified values are resolved at startup and held only in memory. They don't appear in logs (zeroized).

## Inspecting the Config

```bash
senko config                    # current merged config, as JSON
senko config --output text      # human-readable output
senko doctor                    # config + hook + migration sanity check
```

## Common Mistakes

- **Old `[hooks.*]` format** — removed in v1. Rewrite as `[cli.task_*]` / `[server.*]`. Legacy config is not auto-migrated.
- **`pre_hooks` / `post_hooks` arrays** — removed. Use `hooks.<name>.when = "pre" | "post"` instead.
- **`on_no_eligible_task` event** — removed. Replace with `[cli.task_select.hooks.<name>]` + `on_result = "none"`.
- **Forgetting the runtime prefix** — `[task_add.hooks.*]` without a runtime prefix belongs to no section and will not fire.
