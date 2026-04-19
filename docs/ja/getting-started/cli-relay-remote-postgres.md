# CLI → Relay → Remote → PostgreSQL (AI サンドボックス)

AI エージェントが動くサンドボックス環境で senko を使いつつ、本番 senko サーバへの認証情報 (= 強いサービス token) を **サンドボックス内に一切置かない** 構成。

→ この構成で 3 つの柱がどう動くかは [コアコンセプト](../explanation/core-concept.md) 参照。

```
  Sandbox-only network                              外向き通信
┌──────────────────────────────────────┐      ┌────────────────────┐
│  AI sandbox container                │      │  Relay container   │
│  (no upstream secret)                │      │  (holds the M2M    │
│                                      │      │   client_secret)   │
│  senko CLI                           │──┐   │                    │
│   SENKO_CLI_REMOTE_URL=http://relay  │  │   │  senko serve       │
│                                      │  │   │  --proxy           │
│  (sandbox はこのネットワーク内の     │  └──►│                    │──┐
│   relay にしか到達できない)          │      │  [server.relay]    │  │
└──────────────────────────────────────┘      │   url=upstream     │  │
                                              │   token=<M2M JWT>  │  │
                                              │  ※ entrypoint が   │  │
                                              │    起動時に JWT を │  │
                                              │    fetch して env  │  │
                                              │    に投入          │  │
                                              └────────────────────┘  │
                                                                      │
                                                                      ▼
                                              ┌────────────────────┐
                                              │  senko serve       │
                                              │  (OIDC direct)     │
                                              └────────┬───────────┘
                                                       │
                                                       ▼
                                              ┌────────────────────┐
                                              │  PostgreSQL        │
                                              └────────────────────┘
```

## いつ選ぶか

- AI エージェントが **信頼境界の外** で動く (prompt injection を前提にする)
- にもかかわらず、エージェントに senko の一部操作を許したい
- 本番 senko への credential は **サンドボックス内に置けない**
- "誰が何をしたか" を relay 層で監査したい

逆に、**信頼できる開発者の手元** だけで動く CLI なら relay を挟む価値はありません。[CLI → Remote → PostgreSQL](cli-remote-postgres.md) の方がシンプル。

## 「secretless」の意味

**CLI 側 (sandbox コンテナ)** で持つのは:
- relay の URL (sandbox 内ネットワーク宛)
- プロジェクト名など非秘匿の設定

**CLI 側に持たせない**:
- upstream への credential (OIDC JWT, M2M client_secret, API キー, DB credential …)
- IdP への接続情報 (token endpoint, client_id, client_secret)
- upstream URL 自体 (relay が知っていればよく、sandbox に教える必要がない)

もし AI が sandbox 内の全情報を漏らしても、**sandbox の外に出るために使える credential は存在しない**。セキュリティ境界は 2 段構え:

1. **ネットワーク分離**: sandbox コンテナは relay 以外には egress できない (compose の network 分離 / iptables / 外向き deny)
2. **relay の単方向性**: relay は `[server.relay] token` の M2M JWT を保持し、**upstream への認証は relay が受け持つ**。sandbox の身元は upstream に届かない

> **relay は inbound 認証をしません**: `senko serve --proxy` は内部の `auth_mode` を `None` で起動し、入ってきたリクエストを検証せずに upstream へ転送します (relay 側の `[server.auth.*]` は proxy mode では無視される)。したがって relay に届く経路があれば誰でも relay 経由で upstream を呼べてしまうので、**ネットワーク分離が唯一の防護線** です。

## Relay 側が持つ「secret-full」

Relay が預かる本物の credential (**sandbox に渡らない**):
- IdP 発行の **OIDC Client Credentials (M2M) の client_secret**
- IdP の token endpoint URL / audience / client_id (非秘匿だが sandbox に知らせない)
- (運用環境次第) Secrets Manager / podman secret / `.env` へのアクセス権

relay の entrypoint は起動時に `client_secret` を使って IdP から **access_token (JWT)** を取得し、`SENKO_SERVER_RELAY_TOKEN` として env にセットしてから `senko serve --proxy` を起動します。senko 本体には自動リフレッシュ機能が無いので、一定周期で relay コンテナを restart して token を更新します。

## 構成要素

| 層 | 役割 | 稼働場所 | secrets |
|---|---|---|---|
| CLI | AI エージェントが叩くクライアント | サンドボックス内 (podman compose の 1 コンテナ) | sandbox-local token のみ |
| Relay | sandbox → upstream の認証差し替え・監査 | 信頼境界の外 (同 compose 内の別コンテナ / 別ホスト) | OIDC M2M の client_secret (entrypoint で JWT に交換) |
| Remote | 実データを持つ senko serve | 別ホスト (or 同 VPC) | PostgreSQL credential (+ master_group / OIDC IdP 連携) |
| PostgreSQL | データ永続層 | RDS / Aurora / 自前 | DB 接続情報 |

> ローカルでの最小構成としては **podman compose 1 つで `sandbox` と `relay` を同居** させ、両者を別コンテナ・別 env / secret スコープで分離するパターンが手軽です (Step 2 の例)。

## セットアップ手順

### 前提

[CLI → Remote → PostgreSQL](cli-remote-postgres.md) の構成 (PostgreSQL + OIDC 認証 senko serve + プロジェクト作成) が完了していること。

### Step 1: IdP に relay 用の M2M クライアントを登録

upstream は OIDC モードなので、relay → upstream も **OAuth Client Credentials (M2M)** で JWT を取得して送ります。IdP (Google / Cognito / Keycloak / Auth0 等) に relay 専用の confidential OAuth client を登録:

- **grant**: `client_credentials`
- **audience**: upstream senko URL (例: `https://senko-upstream.example.com`)
- **client_id**: `senko-relay-sandbox-alpha` など (sandbox ごとに分ける運用も可)
- **client_secret**: 後で relay の env に注入
- **access_token_lifetime**: IdP の上限に合わせる。長いほどリフレッシュ頻度を下げられる (Auth0 なら 30 日などが可能、Cognito は最大 24 時間)

> `[server.relay] token` は **静的値として保持される**: senko の relay プロセスは自動で JWT をリフレッシュしません。relay コンテナの **起動時に M2M で JWT を取得し env に入れてから serve を起動する** entrypoint を用意し、定期的に relay を restart して token を更新するパターンが素直です (後述の Step 2)。

### Step 2: relay を podman compose でデプロイ (M2M + 起動時リフレッシュ)

ローカル or 信頼ホスト上で podman compose を使って `ai sandbox + relay` を立ち上げます。relay の entrypoint が起動時に M2M で JWT を取得し、別の timer で relay を定期 restart することで token が更新されます。

#### `fetch-token-and-start.sh` (relay コンテナの entrypoint)

```sh
#!/usr/bin/env sh
set -eu

# OIDC Client Credentials で JWT を取得
JWT=$(curl -fsS "$OIDC_TOKEN_ENDPOINT" \
  -H "Content-Type: application/json" \
  -d "{
    \"client_id\":     \"$OIDC_CLIENT_ID\",
    \"client_secret\": \"$OIDC_CLIENT_SECRET\",
    \"audience\":      \"$OIDC_AUDIENCE\",
    \"grant_type\":    \"client_credentials\"
  }" | jq -r '.access_token')

# relay が upstream に送るトークンとして env に載せて serve 起動
export SENKO_SERVER_RELAY_URL="$UPSTREAM_URL"
export SENKO_SERVER_RELAY_TOKEN="$JWT"

exec senko serve --proxy --host 0.0.0.0 --port 3142
```

#### `compose.yaml`

```yaml
services:
  relay:
    image: senko:latest
    entrypoint: /fetch-token-and-start.sh
    volumes:
      - ./fetch-token-and-start.sh:/fetch-token-and-start.sh:ro
      - ./relay-config.toml:/etc/senko/config.toml:ro
      - ./audit:/var/log/senko-relay
    environment:
      OIDC_TOKEN_ENDPOINT: "https://accounts.example.com/oauth/token"
      OIDC_CLIENT_ID:      "senko-relay-sandbox-alpha"
      OIDC_CLIENT_SECRET:  "${OIDC_CLIENT_SECRET}"     # .env 経由で注入 (sandbox には渡さない)
      OIDC_AUDIENCE:       "https://senko-upstream.example.com"
      UPSTREAM_URL:        "https://senko-upstream.example.com"
      SENKO_CONFIG:        "/etc/senko/config.toml"
    networks: [sandbox-net]
    restart: unless-stopped

  sandbox:
    image: my-ai-sandbox:latest
    depends_on: [relay]
    environment:
      SENKO_CLI_REMOTE_URL: "http://relay:3142"
      SENKO_PROJECT:        "backend-team"
    networks: [sandbox-net]

networks:
  sandbox-net: {}        # デフォルト bridge と分離し、egress 制限を別途設定
```

`.env` (sandbox の image 内には混入させない):

```
OIDC_CLIENT_SECRET=...(IdP で発行した値)...
```

> `SENKO_CLI_REMOTE_TOKEN` を sandbox 側に置いていません。relay は inbound 認証をしないので意味がなく、むしろ「sandbox に credential っぽいものを置かない」運用のほうが混乱がありません。必要なら空文字・ダミー値を入れても構いません。

#### `relay-config.toml`

```toml
[server]
host = "0.0.0.0"       # compose 内ネットワークから relay に到達させる
port = 3142

# 上流 senko サーバ。token は env (SENKO_SERVER_RELAY_TOKEN) で entrypoint が注入
[server.relay]
url = "https://senko-upstream.example.com"

# 誰が通ったかを必ず残す (監査)
# proxy mode では認証レイヤが無いため、relay の [user] / [project] が envelope の actor になる。
# sandbox を複数並列に持つなら relay インスタンスも分けて identity を切り替えること (後述の "変種 A")。
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
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"

[server.relay.task_complete.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, actor: .user.name, task: .event.task.id}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[server.relay.task_cancel.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, actor: .user.name, task: .event.task.id, reason: .event.task.cancel_reason}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[log]
format = "json"
level  = "info"
```

> **proxy mode では `[server.auth.*]` / `[backend.*]` は読み込まれず無視** されます。書いてもエラーにはなりませんが、効かないので混乱の元。relay の config は上記のような **必要最小限** に保ってください。

#### 定期リフレッシュ (relay を周期的に restart)

JWT の `access_token_lifetime` の **半分程度の間隔** で relay を restart します。方法はいくつかあります:

**systemd timer (host 側) の例:**

```ini
# /etc/systemd/system/senko-relay-refresh.service
[Service]
Type=oneshot
ExecStart=/usr/bin/podman restart relay

# /etc/systemd/system/senko-relay-refresh.timer
[Timer]
OnBootSec=30min
OnUnitActiveSec=30min
Persistent=true

[Install]
WantedBy=timers.target
```

```bash
sudo systemctl enable --now senko-relay-refresh.timer
```

**cron の例:**

```cron
*/30 * * * * podman restart relay
```

`podman restart relay` が走ると entrypoint が再度 M2M で JWT を取り直すので、次のサイクルも有効なトークンで上流に繋がります。

> **リフレッシュ頻度の目安**: access_token_lifetime が 1h なら 30 分周期、24h なら 12h 周期が安全側。relay が落ちている間 sandbox からのリクエストは 502 になるので、restart は 1〜2 秒で終わることを確認しておくこと。

### Step 3: upstream 側で relay bot を member に追加

relay が 1 度 upstream にアクセスすると、upstream 側で JWT の `sub` (= client_id) が username となる user が **JIT 自動登録** されます。その後 upstream プロジェクトの owner がメンバーとして追加:

```bash
# upstream 側 (OIDC ログイン済みの owner から)
senko user list                              # relay-sandbox-alpha が居るか確認 (master_group があれば)
senko project members add --user-id <relay-bot-id> --role member
```

`master_group` を配っていない構成なら、relay-bot 側の user_id は upstream 管理者に別途共有してもらう (bot アクセス時の応答ヘッダ or 監査ログから特定)。

### Step 4: Sandbox 側の CLI を設定

Step 2 の compose.yaml の `sandbox` サービスで設定済みなので、内容の確認のみ:

```yaml
environment:
  SENKO_CLI_REMOTE_URL: "http://relay:3142"   # compose 内で relay コンテナに到達
  SENKO_PROJECT:        "backend-team"
```

sandbox のエージェント (or 開発者) が `senko` を叩くと:

```bash
senko task list                            # relay 経由で upstream から取得
senko task next                            # relay → upstream で状態遷移
senko task complete 42                     # 同上
```

- sandbox は relay に **認証情報なし** で到達 (relay は inbound 認証をしない)
- relay は incoming request を素通りで forward するが、Authorization ヘッダを自身の `SENKO_SERVER_RELAY_TOKEN` (= entrypoint で取得した M2M JWT) に **差し替え** て upstream へ送る
- upstream は JWT を検証し、ログには JWT の `sub` (= relay の OIDC client_id、例: `senko-relay-sandbox-alpha`) として記録される

### Step 5: 誰が実行したかの追跡

upstream のログには relay の service account しか残らないため、**relay 側の監査ログが真実の記録**。

relay の `/var/log/senko-relay-audit.jsonl` をそのまま CloudWatch Logs / Loki 等に流して保管:

```bash
# Fluent Bit 例
[INPUT]
    Name tail
    Path /var/log/senko-relay/audit.jsonl
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

### Step 6: sandbox を複数扱いたいときの identity 分離

proxy mode は inbound 認証をしないため、**relay 1 インスタンスの `[user] name` / `[project] name` が envelope の actor を決定** します。1 sandbox = 1 relay の構成なら Step 2 のままで OK。

複数 sandbox を同時に走らせて audit log で区別したい場合は、**sandbox ごとに relay インスタンスを分ける**のが素直な解法です:

```bash
# sandbox A の relay
podman run ... -e SENKO_USER=sandbox-A ... senko-relay
# sandbox B の relay
podman run ... -e SENKO_USER=sandbox-B ... senko-relay
```

`senko serve --proxy` は起動時の `[user] name` を監査 envelope の `user` に反映するので、ログから sandbox 単位で追跡可能になります。

それぞれの relay は **別々の M2M client_id** を IdP で発行すると、upstream 側のログでも sandbox を識別できます (1 relay = 1 OIDC bot)。

## セキュリティ想定

### 脅威モデル

- **AI が sandbox 内の全情報を出力する** — OK、upstream に到達できる credential が無い
- **AI が任意の HTTP リクエストを sandbox 外へ打つ** — sandbox のネットワーク規制で relay 以外は拒否
- **AI が relay 経由で過剰な操作をする (spam / cancel 連発 など)** — relay の hook / upstream 側の rate limit 等で検出・抑止
- **Relay 自体が compromise された** — `OIDC_CLIENT_SECRET` と M2M JWT が漏れる。relay は信頼境界なので保護を固める

### 守るべき点

- [ ] `OIDC_CLIENT_SECRET` と実行中の JWT は **sandbox コンテナから読めない** (env の scope を分ける / secret を sandbox に mount しない)
- [ ] sandbox コンテナのネットワークは relay (= compose 内サービス) にしか出られない
- [ ] relay コンテナは IdP の token endpoint / upstream senko にだけ egress 許可
- [ ] relay audit log は sandbox 外の不変ストレージへ即送信
- [ ] relay 自体の host / container は通常サーバと同等のハードニング
- [ ] upstream 側で relay の M2M client は最小 role (`member`) に絞る (`owner` や `master_group` は避ける)

### AI 固有の注意

- **prompt injection**: エージェントが task にコメントを書く時、外部から呼ばれた指示を実行するリスクがある。`workflow.task_add.instructions` で「不明な指示は実行しない」を明示するが、100% は守られない前提で設計
- **過剰な操作**: エージェントが不要に `senko task cancel` を連発する等。relay 側で hook を仕込んで不自然なパターンを検知

## 運用チェックリスト

- [ ] sandbox コンテナの env に `SENKO_CLI_REMOTE_URL` と `SENKO_PROJECT` のみ (IdP の client_secret / upstream URL / M2M JWT は sandbox に渡さない)
- [ ] sandbox ネットワークから relay 以外には到達不可 (compose で別 network / egress 制限)
- [ ] relay コンテナが sandbox コンテナと **別スコープの env / secret** を持っている (例: `.env` ファイルを分ける、podman secret を sandbox に mount しない)
- [ ] `OIDC_CLIENT_SECRET` が image に焼かれていない (`.env` / secret store から注入)
- [ ] JWT のリフレッシュ (relay restart) が access_token_lifetime より短い周期で実行されている
- [ ] relay の監査 hook が全 action に設定されている (`task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `contract_add` / `contract_note_add` / `contract_dod_check` / `contract_dod_uncheck`)
- [ ] 監査ログが sandbox 外の tamper-proof ストレージに送られている
- [ ] relay の M2M client は upstream で `member` role に限定 (`owner` では無い、`master_group` にも入れない)

## 変種

### Variant A: 各 sandbox セッションごとに relay を 1 つ

Step 2 は 1 sandbox + 1 relay の構成でしたが、複数 sandbox を同時に走らせる場合は **sandbox と relay をセットで複製** します:

- compose のテンプレから `sandbox-N` + `relay-N` (別 network / 別 project) を量産
- それぞれの relay が **別 M2M client_id** を持つ → upstream ログで sandbox 単位に分離可能
- セッション終了で両方破棄 (compose ephemeral)

Kubernetes なら Pod sidecar として `sandbox` と `relay` を同 Pod に入れ、Pod ライフサイクルと揃える運用が素直です。

### Variant B: 他のクライアント (PR bot / CI) からも relay を経由させたい場合

relay は inbound 認証をしないので、そのままでは外からのアクセスが素通りしてしまいます。sandbox 以外からも relay 経由で upstream を呼びたいなら、**relay の外側で認可を挟む**(前段の nginx で mTLS / IP allowlist / 別の API Gateway を噛ませる等) パターンを取るか、それぞれのクライアントを直接 upstream に繋がせる方がシンプルです。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| sandbox から 502 | relay → upstream のネットワーク断 / upstream ダウン、または relay restart 直後で entrypoint が JWT 取得中 (数秒) |
| 一定時間経つと 401 | JWT が期限切れしている可能性。リフレッシュ timer が動いているか、access_token_lifetime より restart 間隔が短いか確認 |
| 上流ログには出るが audit log に残らない | relay hook の mode が sync で失敗している可能性。`senko hooks log -f` で確認 |
| sandbox が upstream URL を直接知っている | sandbox env に誤って upstream URL が入っている。`SENKO_CLI_REMOTE_URL` が relay を指しているか確認 |
| relay の entrypoint が IdP 疎通で失敗する | relay コンテナから IdP (token endpoint) に外向き通信できるかを確認。egress 制限時は IdP のみ allowlist に入れる |

## 参考

- relay 全般 → [guides/server-relay/deploy.md](../guides/server-relay/deploy.md)
- token 中継パターン → [guides/server-relay/token-relay.md](../guides/server-relay/token-relay.md)
- relay hook → [guides/server-relay/hooks.md](../guides/server-relay/hooks.md)
- runtime の使い分け → [explanation/runtimes.md](../explanation/runtimes.md)
