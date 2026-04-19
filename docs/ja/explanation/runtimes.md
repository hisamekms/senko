# Runtime の使い分け

senko バイナリは同じ 1 つですが、起動の仕方で **4 つの runtime** として振る舞います。どの runtime で動いているかで、どの設定セクションと hook が "有効" になるかが決まります。

## 4 つの runtime

| Runtime | 起動コマンド | データの置き場所 | config セクション |
|---|---|---|---|
| **cli** | `senko task ...` (`serve` 以外) | ローカル SQLite / remote HTTP | `[cli.*]` |
| **server.remote** | `senko serve` | ローカル SQLite / PostgreSQL | `[server.remote.*]` `[server.auth.*]` `[backend.*]` |
| **server.relay** | `senko serve --proxy` | 上流 (別の `senko serve`) へ転送 | `[server.relay.*]` |
| **workflow** | どの runtime でも発火 (skill が消費) | — | `[workflow.*]` |

> **重要**: 実行中の runtime に **マッチする section 以外の hook は発火しません**。起動時に「mismatch な section がある」旨の警告が出るので、必要な hook がどの section に入っているか必ず確認してください。

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

- **一番よく使う形**。ローカル開発で `senko task add` `senko task next` を叩く時はこの runtime
- `[cli.remote]` を設定するとリモートサーバを backend として使える (SQLite ではなく HTTP 経由で上流の `senko serve` に操作を投げる)
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
- 用途:
  - **AI サンドボックス** — エージェントは外部と直接通信できない環境で、サンドボックス内 relay → 外へ通すパターン
  - **マルチテナント** — 複数組織が各自の relay (に別々の認証ポリシー) を立て、裏で 1 つの remote サーバを共有
  - **トークン中継** — クライアントの OIDC トークンを、relay が持っているサービストークンに差し替えて上流に渡す
- hook はリレーの経路で発火 (監査目的に使うのが主)

### workflow

- **runtime というより "論理ステージ"**。Claude Code skill が `senko config` を読んで、各 stage の instructions / hook を自分の行動に織り込む
- 実 runtime (cli / server.remote / server.relay) のどれで動いていても、workflow 設定は常に読まれ得る
- `workflow.plan.hooks.<name>` のように書く。`prompt` フィールドに書いた文字列が skill の agent instruction に注入される

## 同じ "action" が複数 runtime で発火する?

**しません**。`task_complete` イベントは、動作中の runtime が `cli` なら `[cli.task_complete.hooks.*]` だけ、`server.remote` なら `[server.remote.task_complete.hooks.*]` だけが発火します。

ユースケース別の指針:

| やりたいこと | 置く場所 |
|---|---|
| 開発者のデスクトップ通知 | `[cli.*]` |
| サーバ側の監査ログ / SIEM 連携 | `[server.remote.*]` |
| リレー経由の全リクエストロギング | `[server.relay.*]` |
| Claude Code に "この stage ではこの確認をしてから進め" と指示したい | `[workflow.<stage>].instructions` / `hooks.*.prompt` |

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

- 各 runtime の具体的な設定 → `reference/config/cli.md` / `server-remote.md` / `server-relay.md` / `workflow.md`
- hook の共通仕様 → [reference/hooks.md](../reference/hooks.md)
- デプロイ方法 → `guides/server-remote/deploy.md` / `guides/server-relay/deploy.md`
