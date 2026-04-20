# トークン中継 (Token Relay) パターン

relay が `[server.relay] token` を保持してクライアントの Authorization ヘッダを差し替える運用 (substitution mode) の深掘り。relay 全般の挙動は [relay モードで `senko serve` をデプロイする](deploy.md) を参照。

> **前提**: relay は inbound 認証をしません (`auth_mode: None` 固定)。閉鎖ネットワーク内でしか動かせません。このページは **relay → upstream** の認証経路だけを扱います。

## なぜ token 差し替えが要るか

典型シナリオ: AI サンドボックス内のエージェントが本番 senko の strong credential を持てない。

- sandbox 内にクライアント credential を置きたくない (漏洩した時の影響範囲を relay 境界内に閉じる)
- upstream 側は普通の OIDC / API キーで認証している
- → relay が credential を預かり、upstream リクエストで差し替える

クライアント側は relay に **素で到達** すればよく、credential を持つ必要がない。relay と upstream の間だけが認証された経路になる。

## substitution vs passthrough

| 項目 | substitution (`token` 設定あり) | passthrough (`token` 未設定) |
|---|---|---|
| クライアントの Authorization | **捨てる** | そのまま上流へ透過 |
| 上流に届く identity | relay の 1 identity (全リクエスト共通) | クライアントが送ってきた credential の identity |
| sandbox で credential 秘匿 | 可能 | 不可 (クライアント側で credential が必要) |
| 上流ログの actor | relay 固定 | クライアントごと |
| audit | **relay 側で必須** | 上流側で十分 |

AI サンドボックスは **substitution が適切**。上流を公開 SaaS として共有したい単純な中継は passthrough が便利。

## substitution 用の relay credential を上流に用意する

upstream がどの認証モードかで発行手順・運用コストが変わります。

### upstream が OIDC モードの場合 (本番推奨)

2 つの選択肢があります。

#### 選択肢 1: 人間ユーザの session API キーを流用 (シンプル)

relay を **特定の人間 (例: alice) の代理** として動かす方式。relay 1 台 = 1 人の身代わりになります。

```bash
# 自分の PC で
senko auth login --device-name "alice-relay-sandbox"
senko auth token
# => sk_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

この値を `SENKO_SERVER_RELAY_TOKEN` に入れて relay を起動。

- TTL は upstream の `[server.auth.oidc.session] ttl` に従う (例: 30d)。TTL 内は relay 再起動不要
- 失効時は `senko auth login` やり直し → `senko auth token` で新しい値 → relay restart
- `senko auth revoke <session_id>` で個別失効可
- **upstream ログは alice のアクションとして記録される** — AI のすべての操作が alice の責任として残る

詳細な構築手順: [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)

#### 選択肢 2: IdP に M2M クライアントを登録 (bot identity として分離)

relay を **人間とは別の service account** として動かしたい場合。IdP に OAuth Client Credentials クライアントを登録し、relay が IdP から直接 JWT を取得します。

- grant: `client_credentials`
- audience: upstream の URL
- client_id / client_secret を relay の secret store に保管

relay の entrypoint で IdP に M2M リクエストを送り、返ってきた JWT を `SENKO_SERVER_RELAY_TOKEN` に入れて `senko serve` を起動します。IdP の access_token_lifetime は通常 1 時間程度と短いので、**定期的に relay を restart** して token を更新する必要があります。

upstream 側では relay が最初のリクエストで JWT を送ってくると **JIT で user が自動登録** されます (`username` = JWT の `sub` = client_id)。登録後、upstream プロジェクトの owner が member に追加:

```bash
# upstream 側
senko project members add --user-id <relay-bot-id> --role member
```

**選択肢 1 と 2 の比較**:

| 項目 | 選択肢 1 (session API キー) | 選択肢 2 (M2M JWT) |
|---|---|---|
| 運用コスト | 低 (TTL = 数日〜数十日、手動更新) | 中 (TTL = 1h 程度、定期 restart / refresh 実装が必要) |
| upstream ログの actor | 実在の人間 (alice) | bot identity (例: `senko-relay-sandbox`) |
| 責任の所在 | alice 個人 | service account 単位で分離可 |
| IdP 側の追加設定 | 不要 (通常のユーザログインを流用) | M2M クライアントの登録が必要 |
| relay 側の refresh 実装 | 不要 | あり (entrypoint で IdP 叩き直し + cron/timer で restart) |
| 漏洩時の影響 | alice の member 権限 | service account の member 権限 |

小〜中規模運用や個人の sandbox 用途なら **選択肢 1** が圧倒的に楽。複数 relay を service account 単位で厳密に分離したい / SOC2 等の監査要件で bot identity が必要なら **選択肢 2**。

### upstream が trusted_headers モードの場合

API Gateway が認証を終端しているので、relay は **API Gateway が受理する IdP の JWT (access_token)** を持つ必要があります。senko の session API キーは使えません (senko 側が API キーを発行していないため)。

典型的には選択肢 2 (M2M) と同じ運用になります: IdP から client_credentials で JWT を取得 → `SENKO_SERVER_RELAY_TOKEN` に入れる → 短命なので定期 refresh。人間の JWT を使う選択肢 1 相当も可能ですが、IdP 由来の access_token_lifetime で失効するので運用は選択肢 2 と同じ短命トークン扱いになります。

### upstream が API キーモードの場合 (試用用)

API キーモードの upstream (= 試用構成) に繋ぐ場合は master_key で通常 API キーを発行し、それを `SENKO_SERVER_RELAY_TOKEN` として使います:

```bash
# upstream (API キーモード運用時) で
curl -s -X POST https://senko-upstream.example.com/api/v1/users \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"username":"relay-bot"}'
curl -s -X POST https://senko-upstream.example.com/api/v1/projects/1/members \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"user_id":7,"role":"member"}'
curl -s -X POST https://senko-upstream.example.com/api/v1/users/7/api-keys \
  -H "Authorization: Bearer $UPSTREAM_MASTER_KEY" \
  -d '{"name":"relay-bot"}'
# => key を SENKO_SERVER_RELAY_TOKEN に
```

API キーは長命なので relay 再起動による定期更新は不要です。ただし API キーモードの upstream 自体が試用用途なので、本番では OIDC upstream + 選択肢 1/2 を選んでください。

## 監査の戻し方

substitution mode では upstream ログから実クライアントが特定できません。**relay 側の hook で audit ログを吐く** のが定石:

```toml
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  runtime: .runtime,
  actor: .user.name,
  project: .project.name,
  action: \"task_add\",
  task: .event.task.id
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"
```

ただし **relay は inbound 認証をしないため、envelope の `.user` / `.project` は relay コンテナの `[user] name` / `[project] name` (起動時 env / config) の値** が反映されます。クライアントごとの区別は取れません。

複数クライアントを単一 relay で区別したい場合は相当できません。sandbox ごとに **relay インスタンスを分ける** のが実用的解決策 (各 relay に別 `[user]` env を与える)。

## ヘッダ書き換え (動的 identity mapping) は未対応

クライアント JWT の claim を見て、upstream へ対応する service token に動的切り替える機能は **現状 relay 単体では未対応**。必要なら:

- relay の **前段** に reverse proxy / API Gateway / Lambda を挟んで動的に書き換える
- または upstream 側で `trusted_headers` を有効化し、前段が `x-senko-user-sub` 等を注入する構成

## よくある間違い

- **`[cli.remote]` の URL と `[server.relay]` を混同する** — 前者は CLI (人間/エージェント) が relay に繋ぐ設定、後者は relay 自身の上流設定
- **relay に認証があると思って `[server.auth.api_key]` を書く** — proxy mode では読まれない。認可はネットワーク境界で確保するしかない
- **substitution + passthrough を同時に期待する** — `token` 設定があれば一律 substitution。クライアントの Authorization は捨てられる

## 次のステップ

- relay 全般の運用 → [relay モードで `senko serve` をデプロイする](deploy.md)
- hook 実例 → [`[server.relay.*]` hook の実例](hooks.md)
- AI サンドボックスの end-to-end → [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)
