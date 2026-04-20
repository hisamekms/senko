# Runtime の使い分け

senko バイナリは同じ 1 つですが、起動の仕方で **3 つの runtime** として振る舞います。どの runtime で動いているかで、どの設定セクションと hook が "有効" になるかが決まります。

## 3 つの runtime

| Runtime | 起動コマンド | データの置き場所 | config セクション |
|---|---|---|---|
| **cli** | `senko task ...` (`serve` 以外) | ローカル SQLite / remote HTTP | `[cli.*]` |
| **server.remote** | `senko serve` | ローカル SQLite / PostgreSQL | `[server.remote.*]` `[server.auth.*]` `[backend.*]` |
| **server.relay** | `senko serve --proxy` | 上流 (別の `senko serve`) へ転送 | `[server.relay.*]` |

## 選び方フローチャート

```
Q1. サーバを立てる予定はある？
    │
    ├─ No → [cli] を使う (ローカル SQLite)
    │        → getting-started/local-sqlite.md
    │
    └─ Yes
        │
        Q2. クライアントが直接 DB に繋いでいい？
        │
        ├─ Yes → [server.remote]  (= senko serve)
        │         → getting-started/cli-remote-postgres.md
        │
        └─ No (AI サンドボックス内など、上流サーバへ中継したい)
              → [server.relay]  (= senko serve --proxy)
                 → getting-started/cli-relay-remote-postgres.md
```

## それぞれの位置づけ

### cli

- `senko task add` / `senko task next` などの CLI 操作で動いている時の runtime
- 既定ではローカル SQLite を直接触るが、`[cli.remote]` を設定するとリモートサーバを backend として使える (HTTP 経由で上流の `senko serve` に操作を投げる)
- hook は `[cli.task_add.hooks.<name>]` 等の形で書く
- Claude Code skill も結局は `senko` CLI を叩くので、skill 経由の操作は全部この runtime

### server.remote

- **チーム共有の DB を持つサーバ**。`senko serve` として起動
- SQLite / PostgreSQL を直接読み書きし、REST API を公開
- 認証方式 3 択 (API キー / OIDC / 信頼ヘッダ)
- hook は `[server.remote.task_complete.hooks.audit]` のように、サーバ側で発火させたいものをここに書く
- 例: タスク完了時に SIEM に監査ログを送る、metrics を emit する、Slack 通知する

### server.relay

- **DB を持たず、上流の別サーバへ HTTP 中継するだけ**の薄いサーバ。`senko serve --proxy` で起動
- **inbound 認証機能なし** (`auth_mode` は None 固定)。**閉鎖ネットワーク前提** で運用し、到達可能範囲の限定が実質的な認可になる
- 用途:
  - **AI サンドボックス** — エージェントは外部と直接通信できない環境で、サンドボックス内 relay → 外へ通すパターン
  - **トークン中継 (substitution)** — クライアントが credential を持たず、relay が預かった M2M JWT / API キーに差し替えて上流へ送る
- hook はリレーの経路で発火 (監査目的に使うのが主)
- 外部から直接受ける必要があるなら reverse proxy / API Gateway を前段に置き、そちらで認可する構成にする

## 同じ "action" が複数 runtime で発火する?

**しません**。`task_complete` イベントは、動作中の runtime が `cli` なら `[cli.task_complete.hooks.*]` だけ、`server.remote` なら `[server.remote.task_complete.hooks.*]` だけが発火します。

ユースケース別の指針:

| やりたいこと | 置く場所 |
|---|---|
| 開発者のデスクトップ通知 | `[cli.*]` |
| サーバ側の監査ログ / SIEM 連携 | `[server.remote.*]` |
| リレー経由の全リクエストロギング | `[server.relay.*]` |

## 複合構成の例

### ケース A: 1 人、ローカルのみ

- runtime: `cli`
- config: `.senko/config.toml` に `[cli.*]` hook のみ
- DB: `$XDG_DATA_HOME/senko/projects/<dir>/data.db`

### ケース B: チーム、サーバ共有

- runtime (サーバ側): `server.remote`
  - config: サーバの `.senko/config.toml` に `[server.remote.*]` hook・`[server.auth.oidc]` 等
  - DB: PostgreSQL
- runtime (開発者側): `cli` + `[cli.remote]`
  - config: 開発者ごとの `.senko/config.local.toml` に `[cli.remote] url = ...`
  - DB: リモート経由

### ケース C: AI サンドボックス

- runtime (サンドボックス内): `server.relay`
  - 上流の remote サーバへ HTTP 転送
- runtime (上流): `server.remote`
  - 実際の DB を保持

## 次に読むもの

- 各 runtime の具体的な設定 → `reference/config/cli.md` / `server-remote.md` / `server-relay.md`
- hook の共通仕様 → [reference/hooks.md](../reference/hooks.md)
- デプロイ方法 → `guides/server-remote/deploy.md` / `guides/server-relay/deploy.md`
