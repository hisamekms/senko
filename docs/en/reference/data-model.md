# Data Model

Schema for every table senko persists. The SQLite and PostgreSQL schemas agree (types below use SQLite conventions).

## ER Overview

```
projects (1) ─┬─ (N) tasks ─┬─ (N) task_definition_of_done
              │              ├─ (N) task_in_scope
              │              ├─ (N) task_out_of_scope
              │              ├─ (N) task_tags
              │              ├─ (N) task_dependencies (self-ref)
              │              └─ (0..1) contracts
              │
              ├─ (N) contracts ─┬─ (N) contract_definition_of_done
              │                 ├─ (N) contract_tags
              │                 └─ (N) contract_notes
              │
              ├─ (N) metadata_fields
              │
              └─ (N) project_members ─ (1) users
                                          └─ (N) api_keys
```

## projects

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | AUTOINCREMENT |
| `name` | TEXT | UNIQUE |
| `description` | TEXT? | |
| `created_at` | TEXT | ISO 8601 UTC |

The initial migration inserts `id=1, name='default'` automatically.

## users

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `username` | TEXT | UNIQUE |
| `display_name` | TEXT? | |
| `email` | TEXT? | UNIQUE |
| `sub` | TEXT? | OIDC subject claim (UNIQUE) |
| `created_at` | TEXT | |

Seed: `id=1, username='default'`.

## project_members

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `project_id` | INTEGER FK(projects) | ON DELETE CASCADE |
| `user_id` | INTEGER FK(users) | ON DELETE CASCADE |
| `role` | TEXT | `owner` / `member` / `viewer`, default `member` |
| `created_at` | TEXT | |

UNIQUE(project_id, user_id).

## api_keys

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `user_id` | INTEGER FK(users) | |
| `key_hash` | TEXT | SHA-256 hash (UNIQUE) |
| `key_prefix` | TEXT | Display / identification prefix |
| `name` | TEXT | Freeform label |
| `device_name` | TEXT? | Auto-set on OIDC login |
| `created_at` | TEXT | |
| `last_used_at` | TEXT? | Last-use timestamp |

The plaintext API key is not stored; it's returned only at issue time. Verification happens via `key_hash`.

## tasks

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | Global ID |
| `project_id` | INTEGER FK(projects) | |
| `task_number` | INTEGER | Project-scoped (UNIQUE(project_id, task_number)); CLI display ID |
| `title` | TEXT | |
| `background` | TEXT? | |
| `description` | TEXT? | |
| `plan` | TEXT? | |
| `status` | TEXT | `draft` / `todo` / `in_progress` / `completed` / `canceled` |
| `priority` | INTEGER | 0 (P0) – 3 (P3), default 2 |
| `assignee_session_id` | TEXT? | Set via `task next --session-id` |
| `assignee_user_id` | INTEGER? FK(users) | |
| `created_at` | TEXT | |
| `updated_at` | TEXT | |
| `started_at` | TEXT? | |
| `completed_at` | TEXT? | |
| `canceled_at` | TEXT? | |
| `cancel_reason` | TEXT? | |
| `branch` | TEXT? | git branch name |
| `pr_url` | TEXT? | |
| `metadata` | TEXT? | JSON text (JSONB on Postgres) |
| `contract_id` | INTEGER? FK(contracts) | ON DELETE SET NULL |

## task_definition_of_done

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `task_id` | INTEGER FK(tasks) | ON DELETE CASCADE |
| `content` | TEXT | |
| `checked` | INTEGER | 0/1 |
| `verification_type` | TEXT | `static` / `execution` / `manual` / `unspecified` (default `unspecified`; reserved for rows migrated from before this column existed — new items cannot set it) |
| `verification_method` | TEXT? | Free-text verification procedure declared at registration |
| `verification_note` | TEXT? | Free-text record of how the item was actually verified, written by `dod check --note`; cleared on uncheck |

The index reflects insertion order (1-based when specified from the CLI).

## task_in_scope / task_out_of_scope

| Column | Type |
|---|---|
| `id` | INTEGER PK |
| `task_id` | INTEGER FK(tasks) |
| `content` | TEXT |

## task_tags

| Column | Type |
|---|---|
| `id` | INTEGER PK |
| `task_id` | INTEGER FK(tasks) |
| `tag` | TEXT |

UNIQUE(task_id, tag).

## task_dependencies

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `task_id` | INTEGER FK(tasks) | The depending side |
| `depends_on_task_id` | INTEGER FK(tasks) | The dependency target |

UNIQUE(task_id, depends_on_task_id). Cycles are detected and rejected in the application layer.

## metadata_fields

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `project_id` | INTEGER FK(projects) | |
| `name` | TEXT | Field key |
| `field_type` | TEXT | `string` / `number` / `boolean` |
| `required_on_complete` | INTEGER | 0/1 |
| `description` | TEXT? | |
| `created_at` | TEXT | |

UNIQUE(project_id, name).

## contracts

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `project_id` | INTEGER FK(projects) | |
| `title` | TEXT | |
| `description` | TEXT? | |
| `metadata` | TEXT? | JSON |
| `created_at` | TEXT | |
| `updated_at` | TEXT | |

## contract_definition_of_done

```
id PK / contract_id FK / content / checked (0/1)
  / verification_type / verification_method? / verification_note?
```

Columns carry the same meaning as `task_definition_of_done`.

## contract_tags

```
id PK / contract_id FK / tag    (UNIQUE(contract_id, tag))
```

## contract_notes

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `contract_id` | INTEGER FK(contracts) | ON DELETE CASCADE |
| `content` | TEXT | |
| `source_task_id` | INTEGER? FK(tasks) | ON DELETE SET NULL |
| `created_at` | TEXT | |

## schema_migrations

| Column | Type |
|---|---|
| `version` | INTEGER PK |
| `name` | TEXT |
| `applied_at` | TEXT |

Unapplied migrations run automatically on first start. SQLite and PostgreSQL track version numbers independently (PostgreSQL uses `sqlx`-managed migration files).

## PostgreSQL-Specific Differences

- `metadata`-class columns are `JSONB`; the `task list --metadata key=value` filter is translated to a JSONB query on the server.
- Timestamps are `TIMESTAMPTZ` (SQLite stores ISO 8601 TEXT).
- `ON DELETE CASCADE` and other constraints match on both sides.

## Where Data Lives

| Setup | Location |
|---|---|
| Local SQLite (default) | `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db` (typically `~/.local/share/senko/projects/<dir-name>/data.db`) |
| SQLite (explicit) | `--db-path` / `SENKO_DB_PATH` / `[backend.sqlite] db_path` |
| PostgreSQL | Only when a connection URL is provided (data persists in the DB) |

`<dir-name>` is the project root directory name. Nothing is written inside the project directory, so you don't need to update `.gitignore`.

Project-root resolution: `--project-root` → `.senko/` (legacy marker) → an upward search for `.git/` → current working directory. Installs that previously held `<project>/.senko/data.db` are migrated to the XDG location on first start.
