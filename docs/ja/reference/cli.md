# CLI リファレンス

すべての `senko` サブコマンドの網羅的一覧。

## グローバルオプション

```
--output <FORMAT>       json または text (既定: json)
--project-root <PATH>   プロジェクトルート (省略時は自動検出)
--config <PATH>         設定ファイルのパス (env: SENKO_CONFIG, 既定: .senko/config.toml)
--dry-run               実行せずに出力だけ表示 (状態変更コマンドのみ)
--log-dir <PATH>        ログ出力ディレクトリ (既定: $XDG_STATE_HOME/senko)
--db-path <PATH>        SQLite DB ファイルパス (env: SENKO_DB_PATH)
--postgres-url <URL>    PostgreSQL 接続 URL (env: SENKO_POSTGRES_URL)
--project <NAME>        操作対象プロジェクト (env: SENKO_PROJECT)
--user <NAME>           操作ユーザ (env: SENKO_USER)
--attr KEY=VALUE        trace/baggage に乗せる属性 (繰り返し可、malformed はエラー終了)
```

> グローバルオプションは **サブコマンドの前後どちらに置いてもよい**: `senko --output text task list` と `senko task list --output text` のどちらも受け付けられる。

> `--attr` / `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` の詳細・優先順位・予約 namespace 除外仕様は [Tracing リファレンス](tracing.md) を参照。

## コマンド一覧

| 集約 | サブコマンド |
|---|---|
| Task | `senko task add/list/get/next/ready/start/edit/complete/cancel/dod/deps` |
| Contract | `senko contract add/list/get/edit/delete/dod/note` |
| Project | `senko project list/create/delete/metadata-field/members` |
| User | `senko user list/create/update/delete` |
| Auth | `senko auth login/token/status/logout/sessions/revoke` |
| Hooks | `senko hooks log/test` |
| モード系 | `senko serve` / `senko web` / `senko config` / `senko doctor` / `senko skill-install` |
| 開発用 (`dev-tools` feature 限定) | `senko dev seed [reset\|append]` |

## `senko task`

### `task add`

```bash
senko task add --title "..." [--priority p2] [--background ...] [--description ...] \
               [--definition-of-done ...] [--in-scope ...] [--out-of-scope ...] \
               [--tag ...] [--depends-on <id>] [--branch ...] [--metadata '{...}'] \
               [--assignee-user-id self|<id>]

# JSON から一括作成
echo '{"title":"...", ...}' | senko task add --from-json
senko task add --from-json-file task.json
```

- 新規タスクは `draft` で作成される
- 既定 priority: `p2`
- `--depends-on` は繰り返し可能 (`--depends-on 3 --depends-on 5`)

### `task list`

```bash
senko task list                          # 既定 limit 50
senko task list --status todo            # 状態で絞り込み (繰り返し可)
senko task list --ready                  # todo かつ全依存 completed
senko task list --tag backend            # タグで絞り込み (繰り返し可)
senko task list --contract 42            # Contract で絞り込み
senko task list --metadata "team=backend"
senko task list --id-min 100 --id-max 199
senko task list --limit 20               # limit: 1..=200 既定 50
senko task list --limit 20 --after <cursor>   # cursor で次ページを取得
senko task list --ready --include-unassigned
```

**レスポンス形状 (JSON)**

```json
{
  "items": [ { "id": 1, "title": "...", ... }, ... ],
  "next_cursor": "eyJpZCI6MjB9"
}
```

`next_cursor` が `null` のときは続きがない。`--after <cursor>` に渡すと次ページが取れる。cursor は不透明トークンなので手で decode しないこと。

`--output text` では、`next_cursor` が set の場合にリストの末尾へ `... more: --after <cursor>` が追記される。

### `task get <id>`

タスク詳細 (JSON のみ)。

### `task next`

```bash
senko task next [--session-id <id>] [--metadata '{...}'] [--include-unassigned]
```

ready な中から **priority → created_at → id** 順で 1 件を `in_progress` にする。

### `task publish <id>` / `task start <id>`

手動での状態遷移:

- `task publish`: `draft → todo`
- `task start`: `todo → in_progress`

### `task edit <id>`

```bash
# スカラー更新
senko task edit 1 --title "..." --description "..." --plan "..." --priority p0
senko task edit 1 --branch "feature/x" --pr-url "https://..."
senko task edit 1 --contract 42

# クリア系
senko task edit 1 --clear-description --clear-plan --clear-branch --clear-pr-url
senko task edit 1 --clear-contract --clear-assignee-user-id

# 配列: set / add / remove (tag / definition-of-done / in-scope / out-of-scope)
senko task edit 1 --set-tags "a" "b" "c"
senko task edit 1 --add-tag x --add-tag y
senko task edit 1 --remove-tag old

# Metadata
senko task edit 1 --metadata '{"key":"value"}'          # shallow merge
senko task edit 1 --replace-metadata '{"only":"this"}'  # 全置換
senko task edit 1 --clear-metadata

# 担当者
senko task edit 1 --assignee-user-id self
senko task edit 1 --assignee-user-id 3
```

### `task complete <id>`

```bash
senko task complete 1                # in_progress → completed (DoD 未完了があるとエラー)
senko task complete 1 --skip-pr-check  # merge_via=pr 構成で PR 検証をスキップ
```

### `task cancel <id>`

```bash
senko task cancel 1 [--reason "..."]
```

### `task dod`

```bash
senko task dod check <task_id> <index>     # 1-based index
senko task dod uncheck <task_id> <index>
```

### `task deps`

```bash
senko task deps add <task_id> --on <dep_id>
senko task deps remove <task_id> --on <dep_id>
senko task deps set <task_id> --on <id1> <id2> ...
senko task deps list <task_id> [--limit 20] [--after <cursor>]
```

`task deps list` は `{items, next_cursor}` を返す。cursor セマンティクスは `task list` と同じ。

## `senko contract`

```bash
senko contract add --title "..." [--description ...] [--definition-of-done ...] \
                   [--tag ...] [--metadata '{...}']
senko contract add --from-json / --from-json-file <path>

senko contract list [--tag ...] [--limit 20] [--after <cursor>]
senko contract get <id>

senko contract edit <id> --title ... --description ...
                         --set-tags / --add-tag / --remove-tag
                         --set-definition-of-done / --add-definition-of-done / --remove-definition-of-done
                         --metadata / --replace-metadata / --clear-metadata
                         --clear-description

senko contract delete <id>

senko contract dod check <contract_id> <index>
senko contract dod uncheck <contract_id> <index>

senko contract note add <contract_id> --content "..." [--source-task <task_id>]
senko contract note list <contract_id> [--limit 20] [--after <cursor>]
```

`contract list` と `contract note list` は `{items, next_cursor}` を返す。cursor セマンティクスは `task list` と同じ。

## `senko project`

```bash
senko project list [--limit 20] [--after <cursor>]
senko project create --name <name> [--description ...]
senko project delete <id>
```

### `project metadata-field`

```bash
senko project metadata-field add --name <name> --type string|number|boolean \
                                 [--required-on-complete] [--description ...]
senko project metadata-field list [--limit 20] [--after <cursor>]
senko project metadata-field remove --name <name>
```

### `project members`

```bash
senko project members list [--limit 20] [--after <cursor>]
senko project members add --user-id <id> [--role owner|member|viewer]
senko project members remove --user-id <id>
senko project members set-role --user-id <id> --role owner|member|viewer
```

## `senko user`

```bash
senko user list [--limit 20] [--after <cursor>]
senko user create --username <name> [--sub <oidc-sub>] [--display-name ...] [--email ...]
senko user update <id> [--username ...] [--display-name ...]
senko user delete <id>
```

## `senko auth`

```bash
senko auth login [--device-name <name>]   # OIDC ブラウザログイン → keychain に token 保存
senko auth token                           # 保存済み token を stdout へ (scripting 用)
senko auth status                          # 現在のログイン情報
senko auth logout                          # 現セッション revoke + keychain から削除
senko auth sessions [--limit 20] [--after <cursor>]   # 自分のセッション一覧
senko auth revoke <id>                     # 特定セッション revoke
senko auth revoke --all                    # 全セッション revoke
```

> **ページネーションに関する注記。** 上記の list 系コマンド (`project list` / `project metadata-field list` / `project members list` / `user list` / `auth sessions`) も `task list` と同じく `{items, next_cursor}` を返す。`--output text` では `next_cursor` がある時に末尾へ `... more: --after <cursor>` 行が追記される。`auth sessions` は期限切れセッションを **取得後に in-memory でフィルタ** する都合上、1 ページ分の items が `--limit` より少なくなることがあるので、`next_cursor` が `null` になるまで pagination を続けること。

## `senko hooks`

```bash
senko hooks log [-n 20] [-f] [--clear] [--path]
senko hooks test <event_name> [task_id] [--dry-run]
```

event_name の取り得る値:
`task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel` / `task_select` /
`contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add`

## `senko serve` / `senko web`

```bash
senko serve [--port 3142] [--host 127.0.0.1]            # REST API サーバ
senko web   [--port 3141] [--host 127.0.0.1]            # 読み取り専用 Web ビューア
```

> relay モードで起動するための専用フラグはない。`[server.relay] url` (env: `SENKO_SERVER_RELAY_URL`) が設定された状態で `senko serve` を起動すると自動的に上流へ中継する relay として動く。

環境変数:

| 変数 | 用途 | 既定値 |
|---|---|---|
| `SENKO_PORT` | `web` / `serve` 両方のポート | 3141 (web) / 3142 (serve) |
| `SENKO_HOST` | `web` / `serve` 両方のバインド | 127.0.0.1 |
| `SENKO_SERVER_PORT` | `serve` 専用ポート | 3142 |
| `SENKO_SERVER_HOST` | `serve` 専用バインド | 127.0.0.1 |

## `senko config`

```bash
senko config            # 現在の設定を JSON で表示 (マージ済み)
senko config --init     # テンプレートを stdout に出力
```

## `senko doctor`

設定・hook・マイグレーションの健全性チェック。mismatched runtime の hook や、pre+async+abort の組合せ等を警告として出力。

## `senko skill-install`

```bash
senko skill-install [--output-dir .claude] [--yes] [--force]
```

- 既定: カレントプロジェクト直下の `.claude/skills/senko/` に SKILL.md を配置
- `--yes`: 確認プロンプトをスキップ
- `--force`: 既存の senko 所有ディレクトリを削除してクリーン配置

## `senko dev` (開発用、`dev-tools` feature 限定)

> **公開バイナリには含まれません。** `senko dev` は `cargo build --features dev-tools` でビルドした場合だけ存在します。`cargo install senko` で配布される通常バイナリには登場しません。

### `senko dev seed [reset|append]`

ローカル開発と E2E テスト用に、決定的なサンプルデータ (3 ユーザー / 5 contract / 60 task / 30 依存エッジ / 15 contract note / DoD) を投入します。

```bash
# 既存データを残しつつ未シード状態のときだけ投入する (既定)
senko dev seed
senko dev seed append

# senko 管理テーブルの行を削除してから投入する
# (default project/user (id=1) の bootstrap 行は保持)
senko dev seed reset
```

- **対象 backend**: SQLite と PostgreSQL の両方。リモート (`cli.remote.url`) を指す設定では拒否します。
- **idempotency**: 投入された entity には `seed` タグが付くため、`append` は既存シードを検知して noop で抜けます。
- **reset の破壊性**: `tasks`, `contracts`, `contract_notes`, `task_dependencies`, `task_definition_of_done`, `task_tags`, `metadata_fields`, `api_keys`, `project_members`, `users (id != 1)`, `projects (id != 1)` などを削除します。本番 DB に対しては実行しないでください。
- **書き込み経路**: domain repository を直接呼び出し application service 層をバイパスするため、hook event / event store にシード起源のレコードは残りません。

## 環境変数 (抜粋)

| 変数 | 用途 |
|---|---|
| `SENKO_CONFIG` | 設定ファイルパス |
| `SENKO_PROJECT_ROOT` | プロジェクトルート |
| `SENKO_PROJECT` | 操作対象プロジェクト名 |
| `SENKO_USER` | 操作ユーザ名 |
| `SENKO_DB_PATH` | SQLite DB のパス |
| `SENKO_POSTGRES_URL` | PostgreSQL 接続 URL |
| `SENKO_CLI_REMOTE_URL` | リモートサーバ URL |
| `SENKO_CLI_REMOTE_TOKEN` | リモート接続用 API token |
| `SENKO_SERVER_RELAY_URL` | relay → 上流サーバ URL |
| `SENKO_SERVER_RELAY_TOKEN` | relay → 上流認証 token |
| `SENKO_AUTH_API_KEY_MASTER_KEY` | master API key 直接値 |
| `SENKO_AUTH_API_KEY_MASTER_KEY_ARN` | master API key の AWS Secrets Manager ARN |
| `SENKO_LOG_DIR` | hook ログ出力先 |

## 状態遷移まとめ

```
draft → todo → in_progress → completed
                 ↓
              canceled       (active 状態からはいつでも遷移可)
```

- forward only (逆遷移・自己遷移は拒否)
- `senko task next` は todo → in_progress 専用
- `senko task complete` は in_progress → completed 専用
