# CLI → Relay → Remote → PostgreSQL (AI サンドボックス)

AI エージェントが動くサンドボックス環境で senko を使いつつ、本番 senko サーバへの認証情報 (= 強いサービス token) を **サンドボックス内に一切置かない** 構成。

→ この構成で 3 つの柱がどう動くかは [コアコンセプト](../explanation/core-concept.md) 参照。

```
┌─────────────────────────┐      ┌────────────────────┐      ┌─────────────────┐
│  AI sandbox             │      │  Trusted host      │      │  senko serve    │
│  (secretless)           │      │  (secrets live here│      │  (direct)       │
│                         │      │                    │      │                 │
│  senko CLI              │──┐   │  senko serve      │      │                 │
│    │                    │  │   │  --proxy          │      │                 │
│    │  [cli.remote]      │  │   │                    │      │                 │
│    │  url=localhost     │  │   │  [server.relay]   │      │                 │
│    │  token=sandbox-key │  │   │  url=upstream      │──────►                │
│    │  (not sensitive)   │  └──►│  token=SERVICE_TOK │      │                 │
│    ▼                    │      │  (real credential) │      │                 │
│  127.0.0.1:3142         │      │                    │      │                 │
│  or sandbox-only network│      │  [server.auth.*]   │      │                 │
│                         │      │  = sandbox auth    │      │                 │
└─────────────────────────┘      └────────────────────┘      └────────┬────────┘
                                                                      │
                                                                      ▼
                                                             ┌─────────────────┐
                                                             │  PostgreSQL     │
                                                             └─────────────────┘
```

## いつ選ぶか

- AI エージェントが **信頼境界の外** で動く (prompt injection を前提にする)
- にもかかわらず、エージェントに senko の一部操作を許したい
- 本番 senko への credential は **サンドボックス内に置けない**
- "誰が何をしたか" を relay 層で監査したい

逆に、**信頼できる開発者の手元** だけで動く CLI なら relay を挟む価値はありません。[cli-remote-postgres.md](cli-remote-postgres.md) の方がシンプル。

## 「secretless」の意味

**CLI 側** で持つのは以下だけ:
- relay の URL (localhost / サンドボックス内ネットワーク宛)
- relay を認証するためのトークン — **これは「サンドボックス境界の内側でだけ意味がある」鍵**

**CLI 側で持たない**:
- 本番サーバ (upstream) への API キー / OIDC token
- PostgreSQL credential
- 実 user の identity 情報 (identity は relay で解決)

もし AI が CLI が知る全情報を外に漏らしても、**サンドボックス外からは無意味** であるのがポイント。

## Relay 側が持つ「secret-full」

Relay が預かる本物の credential:
- upstream senko への service account token (`[server.relay] token`)
- 自身の認証用 master key
- (AWS 環境なら) Secrets Manager へのアクセス権

これらはサンドボックスに渡らないので、AI が漏らす可能性はゼロ。

## 構成要素

| 層 | 役割 | 稼働場所 | secrets |
|---|---|---|---|
| CLI | AI エージェントが叩くクライアント | サンドボックス内 | sandbox-local token のみ |
| Relay | sandbox → upstream の認証差し替え・監査 | 信頼ホスト (VPC 内の独立コンテナ) | upstream service token |
| Remote | 実データを持つ senko serve | 別ホスト (or 同 VPC) | master key / DB credential |
| PostgreSQL | データ永続層 | RDS / Aurora / 自前 | DB 接続情報 |

## セットアップ手順

### 前提

[cli-remote-postgres.md](cli-remote-postgres.md) の Step 1〜5 (PostgreSQL 準備、senko serve 起動、master key 設定、プロジェクト作成) が完了していること。

### Step 1: Relay 用の upstream service account を発行

upstream 側で relay 専用のユーザ + API キーを作成:

```bash
# master key で relay 専用ユーザを作る
curl -s -X POST https://senko-upstream.example.com/api/v1/users \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username":"relay-sandbox-alpha"}' | jq .
# => {"id": 7, ...}

# プロジェクト member に追加
curl -s -X POST https://senko-upstream.example.com/api/v1/projects/2/members \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"user_id":7,"role":"member"}'

# API キー発行
curl -s -X POST https://senko-upstream.example.com/api/v1/users/7/api-keys \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"relay-sandbox-alpha"}' | jq .
# => {"key": "sk_UPSTREAM_SERVICE_TOKEN...", ...}
```

この `sk_UPSTREAM_SERVICE_TOKEN` を **relay 側でだけ** 使います。

### Step 2: Relay をデプロイ

Relay は **サンドボックスから到達可能だが、サンドボックス境界の外** に置く (例: 同 VPC の別コンテナ / サイドカー)。

`/etc/senko-relay/config.toml`:

```toml
[server]
host = "0.0.0.0"      # sandbox ネットワークから到達させる
port = 3142

# 上流 senko サーバ (本番 DB)
[server.relay]
url = "https://senko-upstream.example.com"
# token は env (SENKO_SERVER_RELAY_TOKEN) or ARN で注入

# sandbox 側からの認証
# sandbox-local に配るだけの鍵なので「strong secret」ではない扱い
[server.auth.api_key]
master_key = "sandbox-local-admin-key"

# 誰が通ったかを必ず残す (監査)
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  runtime: .runtime,
  actor: .user.name,
  actor_id: .user.id,
  action: \"task_add\",
  task: .event.task.id,
  title: .event.task.title
}" >> /var/log/senko-relay-audit.jsonl
'''
mode = "async"

[server.relay.task_complete.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, actor: .user.name, task: .event.task.id}" >> /var/log/senko-relay-audit.jsonl'
mode = "async"

[server.relay.task_cancel.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, actor: .user.name, task: .event.task.id, reason: .event.task.cancel_reason}" >> /var/log/senko-relay-audit.jsonl'
mode = "async"

[log]
format = "json"
level  = "info"
```

環境変数 (systemd EnvironmentFile 等):

```
SENKO_SERVER_RELAY_TOKEN=sk_UPSTREAM_SERVICE_TOKEN_xxxxxxxxxxxx
```

Relay を起動:

```bash
senko serve --proxy --host 0.0.0.0 --port 3142
```

> **Secrets Manager からの直接解決は `[server.relay]` には未対応**。`[server.auth.api_key].master_key_arn` や `[backend.postgres].url_arn` のような ARN 系キーは relay config には存在しない。AWS 配下で動かす場合は Secrets Manager の値を取得する起動スクリプト (例: `aws secretsmanager get-secret-value ... --query SecretString --output text`) 経由で `SENKO_SERVER_RELAY_TOKEN` に注入するか、ECS Task Definition の secret 参照機能などを使ってください。

### Step 3: Sandbox 側の CLI を設定

sandbox イメージに senko バイナリを同梱しつつ、env で接続先を上書き:

```bash
# sandbox 起動時に注入される env
export SENKO_CLI_REMOTE_URL="http://relay.internal:3142"    # sandbox 内から到達可能な relay
export SENKO_CLI_REMOTE_TOKEN="sandbox-local-admin-key"     # relay を通すだけの鍵
export SENKO_PROJECT="backend-team"
```

sandbox 起動後にエージェント (or 開発者) が `senko` を叩くと:

```bash
senko task list                            # relay 経由で upstream から取得
senko task next                            # relay → upstream で状態遷移
senko task complete 42                     # 同上
```

- sandbox 内の `SENKO_CLI_REMOTE_TOKEN` は漏れても relay 境界以外に影響しない
- relay はこのトークンで認証 → リクエストを `sk_UPSTREAM_SERVICE_TOKEN` で書き換えて upstream へ
- upstream のログには `relay-sandbox-alpha` としてだけ記録される

### Step 4: 誰が実行したかの追跡

upstream のログには relay の service account しか残らないため、**relay 側の監査ログが真実の記録**。

relay の `/var/log/senko-relay-audit.jsonl` をそのまま CloudWatch Logs / Loki 等に流して保管:

```bash
# Fluent Bit 例
[INPUT]
    Name tail
    Path /var/log/senko-relay-audit.jsonl
    Parser json

[OUTPUT]
    Name cloudwatch_logs
    Match *
    region ap-northeast-1
    log_group_name /senko/relay-audit
    log_stream_name relay-$(hostname)
    auto_create_group On
```

- sandbox ID やエージェント名を `user.name` or `project.name` に必ずマッピングする
- ログは **サンドボックス外** に送る。sandbox 内から消せないように

### Step 5: Sandbox 内での識別

`user.name` を sandbox ごとに変えるには、relay に入る前段で sandbox ごとに別 relay インスタンス / 別 project を割り当てるか、relay 自体の user (`[user] name`) を env で切り替えます:

```bash
# sandbox ごとに起動する relay の env
SENKO_USER=sandbox-session-${SESSION_ID}
```

`senko serve --proxy` は起動時の `[user] name` を監査 envelope の `user` に反映するので、ログから sandbox セッション単位で追跡可能。

## セキュリティ想定

### 脅威モデル

- **AI が sandbox 内の全情報を出力する** — OK、sandbox-local key しか無いので損失は relay 境界以内
- **AI が任意の HTTP リクエストを sandbox 外へ打つ** — sandbox のネットワーク規制で relay 以外は拒否
- **AI が relay に大量の不正操作を試みる** — relay の hook / rate limit (外部層) で検出
- **Relay 自体が compromise された** — upstream credential が漏れる。relay は信頼境界なので保護を固める

### 守るべき点

- [ ] `SENKO_SERVER_RELAY_TOKEN` は sandbox 内から **絶対に読めない**
- [ ] sandbox のネットワークは relay (or 限定的な egress) にしか出られない
- [ ] relay audit log は sandbox 外の不変ストレージへ即送信
- [ ] relay 自体の host / container は通常サーバと同等のハードニング
- [ ] upstream 側で relay service account の role を最小に絞る (owner では **ない**、`member` が妥当)

### AI 固有の注意

- **prompt injection**: エージェントが task にコメントを書く時、外部から呼ばれた指示を実行するリスクがある。`workflow.task_add.instructions` で「不明な指示は実行しない」を明示するが、100% は守られない前提で設計
- **過剰な操作**: エージェントが不要に `senko task cancel` を連発する等。relay 側で hook を仕込んで不自然なパターンを検知

## 運用チェックリスト

- [ ] sandbox イメージ内に `SENKO_CLI_REMOTE_URL` / `SENKO_CLI_REMOTE_TOKEN` のみ注入 (他の senko 関連 env は無し)
- [ ] sandbox ネットワークから relay 以外には到達不可 (egress 制限)
- [ ] relay が **sandbox 外** のホストで動いている
- [ ] relay の `SENKO_SERVER_RELAY_TOKEN` が Secrets Manager 等から env で供給され、image に焼き付いていない
- [ ] relay の監査 hook が全 action に設定されている (`task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `contract_add` / `contract_note_add` / `contract_dod_check` / `contract_dod_uncheck`)
- [ ] 監査ログが sandbox 外の tamper-proof ストレージに送られている
- [ ] relay の権限は upstream で `member` role に限定

## 変種

### Variant A: 各 sandbox セッションごとに relay を 1 つ

- Pod sidecar として relay を sandbox と同時起動
- sandbox session ID = relay の `[user] name`
- セッション終了で両方破棄

### Variant B: 共有 relay + sandbox ごとの token

- relay は 1 台
- sandbox ごとに別の short-lived API key を発行 (relay の `users` テーブルに発行)
- sandbox 起動時に key を注入、終了時に revoke

### Variant C: sandbox に OIDC 信号を渡す

- sandbox 入口に API Gateway + Cognito を挟む
- 信頼ヘッダ (`x-senko-user-sub` 等) で identity を relay に伝える
- [guides/server-remote/auth-trusted-headers.md](../guides/server-remote/auth-trusted-headers.md)

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| sandbox から 502 | relay → upstream のネットワーク断 / upstream ダウン |
| 上流ログには出るが audit log に残らない | relay hook の mode が sync で失敗している可能性。`senko hooks log -f` で確認 |
| sandbox が upstream URL を直接知っている | sandbox env に誤って upstream URL が入っている。`SENKO_CLI_REMOTE_URL` が relay を指しているか確認 |
| 違うユーザとして upstream に記録される | relay の `[user]` や env が想定通りか |

## 参考

- relay 全般 → [guides/server-relay/deploy.md](../guides/server-relay/deploy.md)
- token 中継パターン → [guides/server-relay/token-relay.md](../guides/server-relay/token-relay.md)
- relay hook → [guides/server-relay/hooks.md](../guides/server-relay/hooks.md)
- runtime の使い分け → [explanation/runtimes.md](../explanation/runtimes.md)
