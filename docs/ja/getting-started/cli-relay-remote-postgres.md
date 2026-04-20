# CLI → Relay → Remote → PostgreSQL (AI サンドボックス)

AI エージェントが動くサンドボックス環境で senko を使いつつ、本番 senko サーバへの認証情報 (= 強いサービス token) を **サンドボックス内に一切置かない** 構成。

→ この構成で 3 つの柱がどう動くかは [コアコンセプト](../explanation/core-concept.md) 参照。

```
  Sandbox-only network                                  外向き通信
┌──────────────────────────────────────┐      ┌──────────────────────────┐
│  AI sandbox container                │      │  Relay container         │
│  (no upstream secret)                │      │  (holds alice's session  │
│                                      │      │   API key, acts as alice)│
│  senko CLI                           │──┐   │                          │
│   SENKO_CLI_REMOTE_URL=http://relay  │  │   │  senko serve (relay mode)│
│                                      │  └──►│                          │──┐
│  (sandbox は relay にしか egress     │      │  SENKO_SERVER_RELAY_URL  │  │
│   できないネットワーク)              │      │  SENKO_SERVER_RELAY_TOKEN│  │
└──────────────────────────────────────┘      │   = alice の session キー│  │
                                              └──────────────────────────┘  │
                                                                            │
                                                                            ▼
                                              ┌────────────────────┐
                                              │  senko serve       │
                                              │  (OIDC direct)     │
                                              │  → alice として記録│
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

> **relay は inbound 認証をしません**: `[server.relay] url` が設定された `senko serve` は内部の `auth_mode` を `None` 固定で起動し、入ってきたリクエストを検証せずに upstream へ転送します (relay 側の `[server.auth.*]` は relay mode では無視される)。したがって relay に届く経路があれば誰でも relay 経由で upstream を呼べてしまうので、**ネットワーク分離が唯一の防護線** です。

## Relay 側が持つ「secret-full」

Relay が預かる本物の credential (**sandbox に渡らない**):
- 上流 senko サーバで受理される **Bearer トークン** — 現時点の推奨は **人間ユーザが `senko auth login` で取得した session API キー** (後述)
- (運用環境次第) Secrets Manager / podman secret / `.env` へのアクセス権

この Bearer トークンを `SENKO_SERVER_RELAY_TOKEN` env として relay に渡すと、relay は受け取ったリクエストの Authorization をこの値に差し替えて upstream に送ります (substitution mode)。

## 現時点の制約: relay = 1 人のユーザー専用

senko の relay には、**呼び出し元 (sandbox 内の CLI) の identity を upstream に forward する機構がありません**。`on-behalf-of` 的なヘッダも無く、relay が送る Authorization は 1 つだけです。結果として **relay は 1 つの senko ユーザーの身代わり** として動きます。

そのため現状の推奨運用は:

- **relay を特定の人間 (例: alice) 専用に構築する** — alice の session API キーを relay に埋め込む
- upstream 側のログ・監査はすべて "alice のアクション" として記録される
- sandbox 内のエージェントが行った操作も「alice が relay 経由で実行した」扱い。個人のエージェントの痕跡を自分のログにまとめたい個人利用と相性が良い
- チームで 1 つの relay を共有すると全員分のアクションが代表 1 人の名前で混ざるため、**チーム利用では sandbox (= relay) を人ごとに分ける** 運用になる

将来的には caller identity forwarding / OAuth Token Exchange (RFC 8693) / per-sandbox bot 等でこの制約を外すことが考えられますが、現時点では未実装です。

## 構成要素

| 層 | 役割 | 稼働場所 | secrets |
|---|---|---|---|
| CLI | AI エージェントが叩くクライアント | サンドボックス内 (podman compose の 1 コンテナ) | なし (relay に素で到達) |
| Relay | sandbox → upstream の認証差し替え・監査 | 信頼境界の外 (同 compose 内の別コンテナ / 別ホスト) | OIDC M2M の client_secret (entrypoint で JWT に交換) |
| Remote | 実データを持つ senko serve | 別ホスト (or 同 VPC) | PostgreSQL credential (+ master_group / OIDC IdP 連携) |
| PostgreSQL | データ永続層 | RDS / Aurora / 自前 | DB 接続情報 |

> ローカルでの最小構成としては **podman compose 1 つで `sandbox` と `relay` を同居** させ、両者を別コンテナ・別 env / secret スコープで分離するパターンが手軽です (Step 2 の例)。

## セットアップ手順

### 前提

[CLI → Remote → PostgreSQL](cli-remote-postgres.md) の構成 (PostgreSQL + OIDC 認証 senko serve + プロジェクト作成) が完了していること。

> **本手順は upstream が OIDC モードで動いている前提** です。upstream が `trusted_headers` モード (API Gateway 配下など) の場合、relay に入れる token の種類と TTL の扱いが変わります — [trusted_headers モードの場合](#trusted_headers-モードの場合) を後で確認してください。

### Step 1: upstream で session API キーを取得する

1. **upstream のセッション TTL を長めに設定** しておく (`[server.auth.oidc.session] ttl` を例えば `"30d"`)。relay に埋め込んだ token が頻繁に失効しないようにする
2. **自分の PC (sandbox 外) で PKCE ログイン**:

   ```bash
   senko auth login --device-name "relay-for-sandbox"
   ```

   OS の keychain に session API キーが保存される
3. **session API キーを取り出す**:

   ```bash
   senko auth token
   # => sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
   ```

   この値を relay の env `SENKO_SERVER_RELAY_TOKEN` に入れます

> `senko auth token` が返すのは **upstream の OIDC 認証を経て senko サーバが発行した session API キー** (`sk_xxx` 形式) です。CLI は PKCE で IdP から得た JWT を一度だけ upstream の `POST /auth/token` に渡し、upstream はそれを検証して内部で新たに API キー (`api_keys` テーブルに保存) を発行して keychain に返しています。IdP の JWT そのものではありません。TTL は `[server.auth.oidc.session]` で制御され、`senko auth revoke` で個別に失効できます。TTL 内は relay 側でのリフレッシュは不要 (再起動も不要)。
>
> TTL 満了時は手動で: `senko auth login` やり直し → `senko auth token` で新しい値を取得 → relay の env を更新 → relay restart。長めの TTL (例: 30d) にして運用負荷を下げてください。

### Step 1.5: (任意) 複数 relay で個人トークンを分ける

1 人 1 relay 運用で回す場合、`--device-name` を relay 用と通常ログインで分けておくと、後で `senko auth sessions` で一覧し個別に revoke できます:

```bash
# 人間操作用 (既にあれば OK)
senko auth login --device-name "alice-laptop"

# relay 用 (別 session として発行)
senko auth login --device-name "alice-relay-sandbox"
senko auth token > /tmp/relay-token.txt   # → relay の .env に書き込む
```

sandbox を廃止したら `senko auth revoke <id>` で relay 用 session だけ失効できる。

### Step 2: relay を podman compose でデプロイ

`.env` (sandbox の image 内には混入させない、`.gitignore` で除外):

```
SENKO_RELAY_TOKEN=sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx  # Step 1 で取得した session API キー
```

`compose.yaml`:

```yaml
services:
  relay:
    image: senko:latest
    command: ["serve", "--host", "0.0.0.0", "--port", "3142"]
    volumes:
      - ./relay-config.toml:/etc/senko/config.toml:ro
      - ./audit:/var/log/senko-relay
    environment:
      SENKO_CONFIG:              "/etc/senko/config.toml"
      SENKO_SERVER_RELAY_URL:    "https://senko-upstream.example.com"
      SENKO_SERVER_RELAY_TOKEN:  "${SENKO_RELAY_TOKEN}"  # .env 経由で注入
      SENKO_USER:                "alice"                 # audit envelope で relay 識別に使う名前
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
  sandbox-net: {}          # デフォルト bridge と分離し、egress 制限を別途設定
```

> `SENKO_CLI_REMOTE_TOKEN` は sandbox 側に置いていません。relay は inbound 認証をしないので意味がなく、「sandbox に credential を置かない」運用のほうが混乱がありません。

起動:

```bash
podman compose up -d
```

relay と sandbox が立ち上がります。senko は upstream 側で `[server.auth.oidc.session] ttl` が切れない限り、relay を再起動する必要はありません。

#### `relay-config.toml`

```toml
[server]
host = "0.0.0.0"       # compose 内ネットワークから relay に到達させる
port = 3142

# 上流 senko サーバ。token は env (SENKO_SERVER_RELAY_TOKEN) で注入
[server.relay]
url = "https://senko-upstream.example.com"

# 誰が通ったかを audit ログに残す
# proxy mode では認証レイヤが無いため、envelope の .user は relay の [user] / SENKO_USER で決まる。
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  via: .user.name,
  action: \"task_add\",
  task: .event.task.id,
  title: .event.task.title
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"

[server.relay.task_complete.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, via: .user.name, task: .event.task.id}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[server.relay.task_cancel.hooks.audit]
command = 'jq -c ". | {ts: .event.timestamp, via: .user.name, task: .event.task.id, reason: .event.task.cancel_reason}" >> /var/log/senko-relay/audit.jsonl'
mode = "async"

[log]
format = "json"
level  = "info"
```

> **proxy mode では `[server.auth.*]` / `[backend.*]` は読み込まれず無視** されます。書いてもエラーにはなりませんが、効かないので混乱の元。relay の config は上記のような **必要最小限** に保ってください。

#### session トークンの更新

`[server.auth.oidc.session] ttl` が切れると relay の `SENKO_SERVER_RELAY_TOKEN` も無効になり、upstream から 401 が返るようになります。その時の手順:

```bash
# 自分の PC で
senko auth login --device-name "alice-relay-sandbox"
senko auth token                             # 新しい session API キーを表示

# .env を更新して relay を restart
vim .env                                      # SENKO_RELAY_TOKEN を書き換え
podman compose up -d --force-recreate relay   # env を再読込して relay を起動し直し
```

TTL を長めに (30d 等) 設定しておけば、この操作は月 1 回程度で済みます。

### Step 3: Sandbox 側の CLI を確認

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
- relay は incoming request を素通りで forward するが、Authorization ヘッダを `SENKO_SERVER_RELAY_TOKEN` (= alice の session API キー) に **差し替え** て upstream へ送る
- upstream はこの session API キーを検証し、**alice のアクション** としてログに記録する

### Step 4: 誰が実行したかの追跡

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

複数 sandbox を同時に走らせる、もしくは複数人で sandbox を使いたい場合は、**使う人ごとに relay を分ける** のが素直な解法です。各 relay にそれぞれの人間の session API キーを入れる:

```bash
# alice の sandbox 用 relay
alice% senko auth login --device-name "alice-relay-sandbox"
alice% senko auth token > alice-relay/.env      # SENKO_RELAY_TOKEN=...
alice% SENKO_USER=alice podman compose up -d    # alice-relay/ 配下で

# bob の sandbox 用 relay
bob%   senko auth login --device-name "bob-relay-sandbox"
bob%   senko auth token > bob-relay/.env
bob%   SENKO_USER=bob podman compose up -d      # bob-relay/ 配下で
```

relay mode の `senko serve` は起動時の `[user] name` / `SENKO_USER` を監査 envelope の `user` に反映するので、relay 層の audit ログから「どの relay を経由したか」が区別できます。
upstream 側のログでは、各 relay がそれぞれ alice / bob の session API キーで認証するため、実在ユーザの名前で記録されます。

## セキュリティ想定

### 脅威モデル

- **AI が sandbox 内の全情報を出力する** — OK、upstream に到達できる credential が sandbox には無い
- **AI が任意の HTTP リクエストを sandbox 外へ打つ** — sandbox のネットワーク規制で relay 以外は拒否
- **AI が relay 経由で過剰な操作をする (spam / cancel 連発 など)** — relay の hook / upstream 側の rate limit 等で検出・抑止。**AI の操作はすべて所有者 (alice) として upstream に記録される** ため、アカウント所有者の責任で監視する
- **Relay 自体が compromise された** — alice の session API キーが漏れる。relay は信頼境界なので保護を固める。漏洩時は `senko auth revoke` で即座に失効可能

### 守るべき点

- [ ] `SENKO_SERVER_RELAY_TOKEN` (= alice の session API キー) は **sandbox コンテナから読めない** (env の scope を分ける / secret を sandbox に mount しない)
- [ ] sandbox コンテナのネットワークは relay (= compose 内サービス) にしか出られない
- [ ] relay コンテナは upstream senko にだけ egress 許可
- [ ] relay audit log は sandbox 外の不変ストレージへ即送信
- [ ] relay 自体の host / container は通常サーバと同等のハードニング
- [ ] session API キーは relay 専用 device_name (`alice-relay-sandbox` 等) で発行し、通常ログイン用と分離 (漏洩時に relay 用だけ `senko auth revoke` で切り離せる)

### AI 固有の注意

- **prompt injection**: エージェントが task にコメントを書く時、外部から呼ばれた指示を実行するリスクがある。`workflow.task_add.instructions` で「不明な指示は実行しない」を明示するが、100% は守られない前提で設計
- **過剰な操作**: エージェントが不要に `senko task cancel` を連発する等。relay 側で hook を仕込んで不自然なパターンを検知

## 運用チェックリスト

- [ ] sandbox コンテナの env に `SENKO_CLI_REMOTE_URL` と `SENKO_PROJECT` のみ (session API キー / upstream URL は sandbox に渡さない)
- [ ] sandbox ネットワークから relay 以外には到達不可 (compose で別 network / egress 制限)
- [ ] relay コンテナが sandbox コンテナと **別スコープの env / secret** を持っている (`.env` ファイルを分ける、podman secret を sandbox に mount しない)
- [ ] `SENKO_RELAY_TOKEN` が image に焼かれていない (`.env` / secret store から注入)
- [ ] relay 用 session は `--device-name` を人間ログインと分けて発行し、一覧 (`senko auth sessions`) から即座に revoke できる状態にある
- [ ] upstream の `[server.auth.oidc.session] ttl` が組織のポリシーに沿っている (長すぎない / 運用コストに見合う)
- [ ] relay の audit hook が全 action に設定されている (`task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel` / `contract_add` / `contract_note_add` / `contract_dod_check` / `contract_dod_uncheck`)
- [ ] 監査ログが sandbox 外の tamper-proof ストレージに送られている
- [ ] relay を通した AI の挙動は **所有者 (alice) の責任で監視** — sandbox audit と upstream の OIDC session ログを突合せる運用を定義

## trusted_headers モードの場合

upstream が OIDC ではなく `trusted_headers` モードで動いている場合 (例: API Gateway + Cognito + Lambda 構成、[AWS デプロイ](../guides/server-remote/aws-deployment.md) 参照)、relay に入れる token の性質が変わります:

| 項目 | OIDC モード | trusted_headers モード |
|---|---|---|
| `senko auth token` が返すもの | senko が発行した session API キー (`sk_xxx`) | IdP が発行した **JWT (access_token) そのまま** |
| 失効管理 | senko サーバの `api_keys` テーブル + `[server.auth.oidc.session] ttl` | senko は関与しない。IdP の access_token_lifetime に従う |
| 典型 TTL | 設定次第 (例: 30 日) | IdP 既定 (通常 1 時間程度、Cognito なら最大 24 時間) |
| `senko auth revoke` | 個別に失効可能 | 使えない (senko DB に session が無い) |
| Refresh | TTL 内は不要。満了時に `senko auth login` やり直し | 短命なので頻繁に取り直す必要あり |

### 運用上の影響

- **relay の `SENKO_SERVER_RELAY_TOKEN` を頻繁に更新する必要がある**。JWT の残り有効期間を跨いで sandbox が動くと 401 が返る
- 自動化の候補: IdP が **refresh_token** を返す構成なら、relay の起動スクリプトで定期的に token を更新する処理を仕込む (現時点の senko CLI は refresh_token を扱わないので、別途実装が必要)
- IdP に **OAuth Client Credentials (M2M) クライアント** を登録して、relay 起動時に client_credentials で JWT を取得する運用も可能 (従来 relay で使われていた M2M 方式)。人間 session の代わりに M2M service account として upstream に記録される
- **upstream 側で relay の M2M アカウントを作成・招待する必要あり** (JIT 登録された後に owner が member として追加)

結論として、trusted_headers upstream + relay 構成は **セッション管理を senko 外の IdP / スクリプトに委ねる** ため、OIDC 直接モードに比べて運用コストが高くなります。可能なら upstream を OIDC モードに切り替えるか、sandbox から直接 IdP に到達させない設計制約が無い場合はこの構成を避ける方が運用は楽です。

## 変種

### Variant A: 各 sandbox セッションごとに relay を 1 つ

Step 2 は 1 sandbox + 1 relay の構成でしたが、複数 sandbox を同時に走らせる場合は **sandbox と relay をセットで複製** します:

- compose のテンプレから `sandbox-N` + `relay-N` (別 network / 別 project) を量産
- それぞれの relay に **別 `--device-name` で発行した session API キー** を入れる → upstream の `api_keys.device_name` から sandbox 単位に分離可能
- セッション終了で両方破棄 (compose ephemeral)

Kubernetes なら Pod sidecar として `sandbox` と `relay` を同 Pod に入れ、Pod ライフサイクルと揃える運用が素直です。

### Variant B: 他のクライアント (PR bot / CI) からも relay を経由させたい場合

relay は inbound 認証をしないので、そのままでは外からのアクセスが素通りしてしまいます。sandbox 以外からも relay 経由で upstream を呼びたいなら、**relay の外側で認可を挟む**(前段の nginx で mTLS / IP allowlist / 別の API Gateway を噛ませる等) パターンを取るか、それぞれのクライアントを直接 upstream に繋がせる方がシンプルです。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| sandbox から 502 | relay → upstream のネットワーク断 / upstream ダウン |
| 一定期間後に 401 | session API キーの TTL 切れ。`senko auth login` で再取得 → `.env` 更新 → `podman compose up -d --force-recreate relay` |
| 想定と違うユーザで upstream に記録される | `SENKO_SERVER_RELAY_TOKEN` が想定と別人の session キーになっている。`senko auth sessions` と relay の env を突合せ |
| 上流ログには出るが audit log に残らない | relay hook の mode が sync で失敗している可能性。`senko hooks log -f` で確認 |
| sandbox が upstream URL を直接知っている | sandbox env に誤って upstream URL が入っている。`SENKO_CLI_REMOTE_URL` が relay を指しているか確認 |

## 参考

- relay 全般 → [guides/server-relay/deploy.md](../guides/server-relay/deploy.md)
- token 中継パターン → [guides/server-relay/token-relay.md](../guides/server-relay/token-relay.md)
- relay hook → [guides/server-relay/hooks.md](../guides/server-relay/hooks.md)
- runtime の使い分け → [explanation/runtimes.md](../explanation/runtimes.md)
