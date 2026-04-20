# データモデル

senko が保存する全テーブルのスキーマ。SQLite / PostgreSQL 共通です (型表記は SQLite 基準)。

## ER 概略

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

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | AUTOINCREMENT |
| `name` | TEXT | UNIQUE |
| `description` | TEXT? | |
| `created_at` | TEXT | ISO 8601 UTC |

初回マイグレーション時に `id=1, name='default'` が自動挿入される。

## users

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `username` | TEXT | UNIQUE |
| `display_name` | TEXT? | |
| `email` | TEXT? | UNIQUE |
| `sub` | TEXT? | OIDC subject claim (UNIQUE) |
| `created_at` | TEXT | |

初期値: `id=1, username='default'`。

## project_members

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `project_id` | INTEGER FK(projects) | ON DELETE CASCADE |
| `user_id` | INTEGER FK(users) | ON DELETE CASCADE |
| `role` | TEXT | `owner` / `member` / `viewer`、既定 `member` |
| `created_at` | TEXT | |

UNIQUE(project_id, user_id)。

## api_keys

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `user_id` | INTEGER FK(users) | |
| `key_hash` | TEXT | SHA-256 ハッシュ (UNIQUE) |
| `key_prefix` | TEXT | 表示・識別用プレフィックス |
| `name` | TEXT | 任意ラベル |
| `device_name` | TEXT? | OIDC login 時に自動付与 |
| `created_at` | TEXT | |
| `last_used_at` | TEXT? | 最終使用時刻 |

API キーの平文は DB に保存されず、発行時のみ返される。検証は `key_hash` との比較で行う。

## tasks

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | グローバル ID |
| `project_id` | INTEGER FK(projects) | |
| `task_number` | INTEGER | project 内で一意 (UNIQUE(project_id, task_number))。CLI 表示用 |
| `title` | TEXT | |
| `background` | TEXT? | |
| `description` | TEXT? | |
| `plan` | TEXT? | |
| `status` | TEXT | `draft` / `todo` / `in_progress` / `completed` / `canceled` |
| `priority` | INTEGER | 0 (P0) – 3 (P3)、既定 2 |
| `assignee_session_id` | TEXT? | `task next --session-id` で set |
| `assignee_user_id` | INTEGER? FK(users) | |
| `created_at` | TEXT | |
| `updated_at` | TEXT | |
| `started_at` | TEXT? | |
| `completed_at` | TEXT? | |
| `canceled_at` | TEXT? | |
| `cancel_reason` | TEXT? | |
| `branch` | TEXT? | git ブランチ名 |
| `pr_url` | TEXT? | |
| `metadata` | TEXT? | JSON 文字列 (Postgres では JSONB) |
| `contract_id` | INTEGER? FK(contracts) | ON DELETE SET NULL |

## task_definition_of_done

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `task_id` | INTEGER FK(tasks) | ON DELETE CASCADE |
| `content` | TEXT | |
| `checked` | INTEGER | 0/1 |

挿入順で index が決まる (1-based で CLI が指定)。

## task_in_scope / task_out_of_scope

| カラム | 型 |
|---|---|
| `id` | INTEGER PK |
| `task_id` | INTEGER FK(tasks) |
| `content` | TEXT |

## task_tags

| カラム | 型 |
|---|---|
| `id` | INTEGER PK |
| `task_id` | INTEGER FK(tasks) |
| `tag` | TEXT |

UNIQUE(task_id, tag)。

## task_dependencies

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `task_id` | INTEGER FK(tasks) | 依存する側 |
| `depends_on_task_id` | INTEGER FK(tasks) | 依存される側 |

UNIQUE(task_id, depends_on_task_id)。循環は application 層で検出・拒否。

## metadata_fields

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `project_id` | INTEGER FK(projects) | |
| `name` | TEXT | field key |
| `field_type` | TEXT | `string` / `number` / `boolean` |
| `required_on_complete` | INTEGER | 0/1 |
| `description` | TEXT? | |
| `created_at` | TEXT | |

UNIQUE(project_id, name)。

## contracts

| カラム | 型 | 備考 |
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
```

## contract_tags

```
id PK / contract_id FK / tag    (UNIQUE(contract_id, tag))
```

## contract_notes

| カラム | 型 | 備考 |
|---|---|---|
| `id` | INTEGER PK | |
| `contract_id` | INTEGER FK(contracts) | ON DELETE CASCADE |
| `content` | TEXT | |
| `source_task_id` | INTEGER? FK(tasks) | ON DELETE SET NULL |
| `created_at` | TEXT | |

## schema_migrations

| カラム | 型 |
|---|---|
| `version` | INTEGER PK |
| `name` | TEXT |
| `applied_at` | TEXT |

初回起動で未適用のマイグレーションが自動適用される。SQLite と PostgreSQL で version 番号は独立管理 (PostgreSQL は `sqlx` の migration ファイルを参照)。

## PostgreSQL 特有の差異

- `metadata` 系は `JSONB` で、`task list --metadata key=value` フィルタはサーバ側で JSONB クエリになる
- タイムスタンプは `TIMESTAMPTZ` (SQLite は ISO 8601 TEXT)
- `ON DELETE CASCADE` 等の制約は両方で一致

## データの保存先

| 構成 | 場所 |
|---|---|
| ローカル SQLite (既定) | `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db` (= 通常 `~/.local/share/senko/projects/<dir-name>/data.db`) |
| SQLite (明示指定) | `--db-path` / `SENKO_DB_PATH` / `[backend.sqlite] db_path` |
| PostgreSQL | 接続 URL が与えられた場合のみ (DB 側に永続化) |

`<dir-name>` はプロジェクトルートディレクトリ名。プロジェクトディレクトリの直下には何も書き込まれないため `.gitignore` 追加は不要です。

プロジェクトルートの解決は `--project-root` / `.senko/` (レガシー互換のためのマーカー) / `.git/` 上方探索 / カレントディレクトリの順。過去に `<project>/.senko/data.db` を持っていたインストールは初回起動で XDG 側へ自動マイグレーションされます。
