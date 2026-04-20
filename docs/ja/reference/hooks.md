# Hooks リファレンス

Hook は **状態遷移の前後に発火するシェルコマンド** です。すべての runtime で共通の仕組みを使います。

概念面は [イベントドリブンなワークフロー](../explanation/event-driven-workflow.md)、runtime の使い分けは [Runtime の使い分け](../explanation/runtimes.md) を参照。

## キー構造

```
<runtime>.<aggregate>_<action>.hooks.<name>
```

| 要素 | 取り得る値 |
|---|---|
| `<runtime>` | `cli` / `server.remote` / `server.relay` / `workflow` |
| `<aggregate>_<action>` | `task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `task_select` / `contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add` / (workflow のみ: 任意 stage 名) |
| `<name>` | 自由 (alphabet/数字/`_`) |

例:

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'done'"

[server.remote.task_add.hooks.audit]
command = "logger -t senko-audit 'new task'"
mode = "async"

[workflow.plan.hooks.review]
command = "true"
prompt = "plan を human にレビューしてもらってから実装に進む"
when = "pre"
```

## HookDef スキーマ

| フィールド | 型 | 既定 | 説明 |
|---|---|---|---|
| `command` | string | **必須** | `sh -c` で実行されるコマンド |
| `when` | `"pre"` / `"post"` | `"post"` | 状態遷移の前 or 後 |
| `mode` | `"sync"` / `"async"` | `"async"` | sync は完了まで待つ、async は fire-and-forget |
| `on_failure` | `"abort"` / `"warn"` / `"ignore"` | `"abort"` | 非ゼロ終了時の挙動 (下記) |
| `enabled` | bool | `true` | `false` にすると定義は残ったまま発火しない |
| `env_vars` | `EnvVarSpec[]` | `[]` | 必須環境変数の宣言 (下記) |
| `on_result` | `"selected"` / `"none"` / `"any"` | `"any"` | **`task_select` 専用**。それ以外では無視 |
| `prompt` | string | `null` | **`workflow.<stage>.hooks.*` 専用**。skill がエージェント指示として注入する |

### `on_failure` の意味

- `abort`: **sync + pre** のときのみ状態遷移をキャンセル (DomainError::HookAborted)。それ以外の組合せでは `warn` と同じ挙動
- `warn`: 失敗を log に WARN として記録
- `ignore`: 失敗を無視 (INFO のみ)

### `on_result` (task_select 限定)

| 値 | 発火条件 |
|---|---|
| `selected` | `task next` がタスクを選べた時のみ |
| `none` | 選べるタスクが無かった時のみ |
| `any` (既定) | どちらでも |

旧 `on_no_eligible_task` イベントは `task_select` + `on_result = "none"` で置き換え。

## EnvVarSpec

```toml
[[cli.task_complete.hooks.webhook.env_vars]]
name = "WEBHOOK_URL"
required = true
default = "https://example.com/fallback"
description = "タスク完了時に POST する宛先"
```

| フィールド | 型 | 既定 | 意味 |
|---|---|---|---|
| `name` | string | 必須 | 環境変数名 |
| `required` | bool | `true` | 発火時点で未設定 & `default` 未指定なら **hook をスキップ** + warn |
| `default` | string? | — | 未設定時のフォールバック値 |
| `description` | string? | — | 設定ファイル読者向けの説明 |

## Hook envelope (stdin に渡される JSON)

### Task action の envelope

```json
{
  "runtime": "cli",
  "backend": {
    "type": "sqlite",
    "db_file_path": "/home/alice/.local/share/senko/projects/my-project/data.db"
  },
  "project": { "id": 1, "name": "default" },
  "user":    { "id": 1, "name": "default" },
  "event": {
    "event_id": "uuid-v4",
    "event": "task_complete",
    "timestamp": "2026-04-19T12:00:00Z",
    "from_status": "in_progress",
    "task": { ... 省略 ... },
    "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5, "canceled": 0 },
    "ready_count": 2,
    "unblocked_tasks": [{ "id": 3, "title": "...", "priority": "P1", "metadata": null }]
  }
}
```

- `task`: `senko task get` と同スキーマ ([CLI リファレンス](cli.md) 参照)
- `unblocked_tasks`: **`task_complete` のみ** に含まれる。完了により ready に遷移した他タスク
- `stats`: その project の状態別タスク数

### Contract action の envelope

`contract_*` イベントでは外側 envelope は同じだが、内側が変わる:

```json
{
  "runtime": "server.remote",
  "backend": { ... },
  "project": { ... },
  "user":    { ... },
  "event": {
    "event_id": "uuid-v4",
    "event": "contract_note_add",
    "timestamp": "...",
    "contract": { "id": 42, "title": "...", "definition_of_done": [...], "notes": [...], "is_completed": false, ... }
  }
}
```

`from_status` / `stats` / `ready_count` / `unblocked_tasks` は task aggregate 限定なので含まれない。

### `backend` フィールド

| `type` | 追加フィールド |
|---|---|
| `sqlite` | `db_file_path` |
| `postgres` | `connection_url` (host/dbname のみ、credential はマスク) |
| `http` | `api_url` (remote / relay 経由時) |

## 発火タイミングまとめ

| Action | pre が走る瞬間 | post が走る瞬間 |
|---|---|---|
| `task_add` | 作成前 (validated 済) | 作成後 |
| `task_ready` | draft → todo の前 | 後 |
| `task_start` | todo → in_progress の前 | 後 |
| `task_complete` | in_progress → completed の前 (DoD 検証後) | 後 |
| `task_cancel` | canceled 遷移の前 | 後 |
| `task_select` | candidate 決定後、状態変更の前 | 変更後 |
| `contract_add` | 作成前 | 作成後 |
| `contract_edit` | 更新前 | 更新後 |
| `contract_delete` | 削除前 | 削除後 |
| `contract_dod_check/uncheck` | 更新前 | 更新後 |
| `contract_note_add` | 追加前 | 追加後 |

## ランタイム外の hook は発火しない

起動中の runtime にマッチしない section に書かれた hook は **スキップされ、起動時に 1 回 warn が出る** ので、想定通りに動かない時はまず `senko doctor` と起動ログを確認してください。

## ロード時検証

`senko doctor` / サーバ起動時に以下が warning として出る:

- `pre` + `async` + `on_failure = "abort"` — async は abort 不可 (事実上 warn)
- `on_result` が `task_select` 以外に付いている — 無視される

## テスト

単発で手動発火:

```bash
senko hooks test task_complete 3            # 実在タスク 3 を使って envelope を組み立て、同期発火
senko hooks test task_complete --dry-run    # envelope JSON のみを表示 (発火しない)
senko hooks test contract_note_add 42       # contract id 42 を題材に contract hook をテスト
```

## ログ

- 既定出力先: `$XDG_STATE_HOME/senko/` (ユーザ設定で `[log] dir` 上書き可)
- `senko hooks log -f` で tail -f 相当
- `[log] hook_output = "file" | "stdout" | "both"` で hook の stdout/stderr をどこに流すか選択

## 実例

- CLI からの通知 → [`[cli.*]` hook の実例](../guides/cli/hooks.md)
- サーバの監査ログ → [`[server.remote.*]` hook の実例](../guides/server-remote/hooks.md)
- Relay 経由のリクエストロギング → [`[server.relay.*]` hook の実例](../guides/server-relay/hooks.md)
