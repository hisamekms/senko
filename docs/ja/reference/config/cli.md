# `[cli.*]` 設定

ローカル CLI バイナリとして動く時 (= `senko serve` / `senko serve --proxy` 以外) に有効な section。

## `[cli]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `browser` | bool | `true` | `senko auth login` でブラウザを自動起動する |

## `[cli.remote]`

CLI が **ローカル DB ではなくリモートサーバに繋ぐ** 時の接続先。設定するとローカル SQLite / PostgreSQL は使われず、全操作が HTTP 経由で上流サーバに投げられる。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `url` | string | `null` | リモートサーバ URL (例: `https://senko.example.com`) |
| `token` | string | `null` | API キー or OIDC セッショントークン |

env override: `SENKO_CLI_REMOTE_URL` / `SENKO_CLI_REMOTE_TOKEN`

token を config に直書きしたくない場合:

- env 変数で注入
- `.senko/config.local.toml` (git 管理外) に書く
- `senko auth login` でログイン → OS keychain にトークン保存、config には URL だけ記載

## `[cli.<action>.hooks.<name>]`

実行中 runtime が `cli` の時に発火する hook。

action は:

- タスク: `task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `task_select`
- Contract: `contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add`

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'senko: task done'"
mode = "async"
on_failure = "ignore"

[cli.task_select.hooks.prompt_for_add]
command = "echo 'ready なタスクがありません'"
on_result = "none"
```

Hook 各フィールドの詳細は [reference/hooks.md](../hooks.md)。

## 主要な env 変数

| 変数 | 対応するキー |
|---|---|
| `SENKO_CLI_REMOTE_URL` | `[cli.remote] url` |
| `SENKO_CLI_REMOTE_TOKEN` | `[cli.remote] token` |
| `SENKO_PROJECT` | `[project] name` |
| `SENKO_USER` | `[user] name` |
| `SENKO_DB_PATH` | `[backend.sqlite] db_path` |

## よくあるパターン

### 個人開発、ローカル DB + デスクトップ通知

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'done' '$SENKO_TASK_TITLE'"
mode = "async"
```

(`SENKO_TASK_TITLE` は `env_vars` で宣言した場合のみ注入されます — 直接参照したい場合は stdin JSON を `jq` でパースする方が安定)

### リモート接続 (OIDC)

```toml
[cli.remote]
url = "https://senko.example.com"

[cli]
browser = true
```

token は `senko auth login` で keychain に保存する。

### CI で bot 実行

本番運用では OIDC Client Credentials (M2M) で取得した JWT を env で注入します。設定ファイルに書くのは URL のみ:

```toml
[cli.remote]
url = "https://senko.example.com"
```

CI ジョブ:

```bash
# IdP から M2M で JWT 取得
JWT=$(curl -s https://accounts.example.com/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id":     "senko-bot",
    "client_secret": "'"$SENKO_BOT_CLIENT_SECRET"'",
    "audience":      "https://senko.example.com",
    "grant_type":    "client_credentials"
  }' | jq -r '.access_token')

export SENKO_CLI_REMOTE_TOKEN="$JWT"
senko task list --status todo --output json
```

セットアップの全体手順は [auth-oidc.md](../../guides/server-remote/auth-oidc.md) の "CI / bot (OAuth Client Credentials / M2M)" 節。試用時の API キー運用は [auth-api-key.md](../../guides/server-remote/auth-api-key.md)。
