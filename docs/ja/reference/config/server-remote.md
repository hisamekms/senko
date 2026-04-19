# `[server.*]` / `[backend.*]` / `[server.auth.*]` 設定

`senko serve` (direct mode) として動く時に有効な section。

## `[server]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `host` | string | `127.0.0.1` | バインドアドレス |
| `port` | u16 | `3142` | ポート |

env override: `SENKO_SERVER_HOST` / `SENKO_SERVER_PORT` (または `SENKO_HOST` / `SENKO_PORT` で `web` と兼用)

## `[backend.sqlite]`

SQLite で動かす時の設定。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `db_path` | string | 自動 | DB ファイルパス。既定: `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db` (通常は `~/.local/share/senko/projects/<dir-name>/data.db`)。レガシーな `<project_root>/.senko/data.db` は初回検知時に XDG 側へマイグレーションされる |

## `[backend.postgres]` (postgres feature 必要)

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `url` | string | `null` | 接続 URL。`postgres://user:pass@host:port/dbname?sslmode=require` 形式 |
| `url_arn` | string | `null` | URL を AWS Secrets Manager から取得する場合の ARN (`aws-secrets` feature 必要) |
| `rds_secrets_arn` | string | `null` | RDS 形式の JSON secret の ARN (`username`/`password`/`host` 必須、`port`/`dbname` オプション) |
| `sslrootcert` | string | `null` | TLS root 証明書のパス |
| `max_connections` | u32 | sqlx の既定 | 接続プール上限 |

env override: `SENKO_POSTGRES_URL`

> `url` と `rds_secrets_arn` を同時に指定した場合、ARN 側が優先されます。

> **重要: 認証モードは pairwise 排他**
>
> `[server.auth.api_key]`(`master_key`) / `[server.auth.oidc]` / `[server.auth.trusted_headers]` は **同時に 1 つだけ** しか有効化できません。複数設定されていると起動時に bail します。master 権限の与え方はモードごとに異なる機構です (下記各 section 参照)。

## `[server.auth.api_key]`

API キー認証を有効化する。`master_key` か `master_key_arn` のどちらかが設定されていれば **API キー認証モード** になる (他モードと排他)。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `master_key` | string | `null` | master key 直接値 |
| `master_key_arn` | string | `null` | master key の AWS Secrets Manager ARN |

env override: `SENKO_AUTH_API_KEY_MASTER_KEY` / `SENKO_AUTH_API_KEY_MASTER_KEY_ARN`

master key の性質 (**API キーモード固有**):

- どの User にも紐づかない特権キー。`master_key` の値を所持して Bearer で送ると `is_master = true` で認証される
- `POST /api/v1/users` 等のブートストラップ API に使う
- 通常の API 操作には個別 API キー (`POST /users/{id}/api-keys` で発行) を使用する
- OIDC / 信頼ヘッダモードには `master_key` 概念は無い。代わりに各モードの `master_group` を使う

## `[server.auth.oidc]`

OIDC 認証モード (他モードと排他)。**初回認証時にユーザが JIT 自動登録** される。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `issuer_url` | string | `null` | IdP の issuer URL |
| `client_id` | string | `null` | PKCE 用 client_id (senko CLI からのログインで使う) |
| `scopes` | string[] | `["openid","profile"]` | リクエストする scope |
| `username_claim` | string | `null` | username として使う JWT claim。未指定時のフォールバック: `preferred_username` → `email` → `sub` |
| `required_claims` | map | `{}` | 必須 claim (key=value の一致)。全 claim が一致しないと認証失敗 |
| `groups_claim` | string | `"groups"` | `master_group` 判定に使う JWT claim 名 (配列型 claim) |
| `master_group` | string | `null` | この値が `groups_claim` の配列に含まれる JWT は `is_master=true` になる。未指定なら super-admin 不在 |
| `callback_ports` | string[] | `[]` | CLI ログイン時の callback ポート。個別 (`"8400"`) も range (`"9000-9010"`) も可 |

## `[server.auth.oidc.session]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `ttl` | string | `null` | 絶対 TTL (例: `"24h"`, `"30d"`) |
| `inactive_ttl` | string | `null` | 無活動 TTL (例: `"7d"`) |
| `max_per_user` | u32 | `null` | ユーザあたり最大セッション数 |

`null` は無制限。

## `[server.auth.trusted_headers]`

**API Gateway 等のリバースプロキシ配下** で使うヘッダベース認証。senko 自身はトークン検証を行わず、ヘッダ値を無条件に信頼します。

> **⚠️ セキュリティ警告**
>
> trusted_headers モード中、senko を **直接インターネットに公開しないでください**。API Gateway 等が唯一の入口になっている前提で、クライアントが直接ヘッダを送れる経路があってはいけません。

他モードと排他。**初回アクセスで user が JIT 自動登録** される (OIDC と同様)。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `subject_header` | string | `null` | **設定すると trusted_headers モードが有効化**。sub を運ぶヘッダ名 |
| `name_header` | string | `null` | display name |
| `display_name_header` | string | `null` | name_header 不在時のフォールバック |
| `email_header` | string | `null` | email |
| `groups_header` | string | `null` | カンマ区切りのグループ名を運ぶヘッダ |
| `master_group` | string | `null` | `groups_header` に含まれる値に一致すると `is_master=true` になる。未指定なら super-admin 不在 |
| `scope_header` | string | `null` | OAuth scope |
| `oidc_issuer_url` | string | `null` | `GET /auth/config` で返す (CLI ログイン用) |
| `oidc_client_id` | string | `null` | 同上 |

## `[server.remote.<action>.hooks.<name>]`

`senko serve` (direct) で状態遷移が起きた時に発火する hook。action 一覧は [reference/config/cli.md](cli.md) と同じ。

```toml
[server.remote.task_complete.hooks.audit]
command = "logger -t senko-audit 'task complete'"
mode = "async"

[server.remote.task_complete.hooks.metrics]
command = "curl -X POST $METRICS_URL -d 'task_complete=1'"
mode = "async"

[[server.remote.task_complete.hooks.metrics.env_vars]]
name = "METRICS_URL"
required = true
```

Hook のスキーマは [reference/hooks.md](../hooks.md)。

## 認証モードの排他性

`api_key` / `oidc` / `trusted_headers` は **同時に 1 つだけ有効**。複数設定した場合は起動時エラーになります (起動せず)。切り替えたい場合は片方のキーを削除してください。
