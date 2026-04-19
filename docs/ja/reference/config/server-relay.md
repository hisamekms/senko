# `[server.relay.*]` 設定

`senko serve --proxy` (relay mode) として動く時に有効な section。

relay サーバは DB を持たず、受け取った API リクエストを上流の direct サーバへ HTTP 転送します。詳細: [explanation/runtimes.md](../../explanation/runtimes.md)

> **重要**: relay mode では `auth_mode` が **None 固定** になり、**`[server.auth.*]` は読み込まれず無視** されます。つまり relay は **inbound 認証を一切行わない**。閉鎖ネットワーク内で運用し、到達可能範囲の限定を実質的な認可とする設計です。公開したい場合は reverse proxy / API Gateway を前段に置いて認可をそこで済ませてください。

## `[server]`

`[server]` は direct / relay で共通。host/port の設定。[server-remote.md](server-remote.md) 参照。

## `[server.relay]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `url` | string | `null` | **必須**。上流 direct サーバ URL。`senko serve --proxy` に必要 |
| `token` | string | `null` | 上流 senko に送る Bearer 値 (= 上流で受理される credential)。未設定ならクライアントの Authorization ヘッダを透過 |

env override: `SENKO_SERVER_RELAY_URL` / `SENKO_SERVER_RELAY_TOKEN`

> `token_arn` のような AWS Secrets Manager 参照は `[server.relay]` には実装されていません。Secrets Manager から取得する場合は起動スクリプトで `aws secretsmanager get-secret-value ...` を実行して `SENKO_SERVER_RELAY_TOKEN` に env として注入してください。

### token の挙動

| 設定 | relay の挙動 |
|---|---|
| `token` 設定あり | **substitution mode**。全上流リクエストで Authorization をこの値に差し替える (クライアントの token は捨てる) |
| `token` 設定なし | **passthrough mode**。クライアントから来た `Authorization` ヘッダをそのまま上流へ透過 |

## `[server.auth.*]` (無効)

**proxy mode では `[server.auth.api_key]` / `[server.auth.oidc]` / `[server.auth.trusted_headers]` はすべて読み込まれません**。書いても起動エラーにはなりませんが、認証には使われないので書かないことを推奨します。

relay の inbound を守りたい場合は:

- 閉鎖ネットワーク (sandbox-only / VPC 内 / loopback) でしか listen させない
- 前段に reverse proxy (nginx / Caddy / ALB / API Gateway) を置いて、そこで IP allowlist / mTLS / JWT 検証等を行う

## `[server.relay.<action>.hooks.<name>]`

relay 経路で状態遷移 API が **上流への転送に成功した後** に発火する hook。

```toml
[server.relay.task_add.hooks.request_log]
command = "jq -c '.event.task | {id, title}' >> /var/log/senko-relay/request.jsonl"
mode = "async"

[server.relay.task_complete.hooks.audit]
command = "logger -t senko-relay 'task complete'"
mode = "async"
```

hook envelope の `.user` / `.project` は **relay コンテナ自身の `[user]` / `[project]` 設定値** が反映されます (relay は認証しないためクライアント単位の identity は取れない)。sandbox などを区別したい場合は relay インスタンスを分けて別の `[user] name` を与えます。

## Relay を使うべきでないケース

- ただ HTTP プロキシが欲しいだけ → nginx / Caddy の reverse proxy で十分
- クライアント → 上流の素直な接続ができる → direct サーバに直接繋ぐ方が低レイテンシ
- クライアントごとに認証を変えたい → relay は inbound 認証をしないので relay 単体では不可能。前段で認可するか、クライアントごとに別 relay インスタンスを立てて分ける

relay が活きるのは、**送信元ネットワークから上流へ直接到達できない** かつ **認証の差し替え (サービストークン化) が必要** なケース。

## 最小構成例

```toml
[server]
host = "127.0.0.1"              # 閉鎖ネットワーク内でのみ listen
port = 3142

[server.relay]
url   = "https://senko-upstream.example.com"
token = "..."                   # env SENKO_SERVER_RELAY_TOKEN で注入するのが普通

[server.relay.task_complete.hooks.audit]
command = 'jq -c "." >> /var/log/senko-relay/audit.jsonl'
mode    = "async"
```
