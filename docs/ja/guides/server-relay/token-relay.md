# トークン中継 (Token Relay) パターン

relay が `[server.relay] token` を保持してクライアントの Authorization ヘッダを差し替える運用 (substitution mode) の深掘り。relay 全般の挙動は [deploy.md](deploy.md) を参照。

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

upstream がどの認証モードかで発行手順が変わります。

### upstream が OIDC モードの場合 (推奨)

IdP に **OAuth Client Credentials (M2M)** クライアントを登録:

- grant: `client_credentials`
- audience: upstream の URL
- client_id / client_secret を relay の secret store に保管

relay の entrypoint で IdP に M2M リクエストを送り、返ってきた JWT を `SENKO_SERVER_RELAY_TOKEN` に入れて `senko serve --proxy` を起動します。JWT は短命なので **定期的に relay を restart** して更新 (実装サンプルは [CLI → Relay → Remote → PostgreSQL: Step 2](../../getting-started/cli-relay-remote-postgres.md) にあります)。

upstream 側では relay が最初のリクエストで JWT を送ってくると **JIT で user が自動登録** されます (`username` = JWT の `sub` = client_id)。登録後、upstream プロジェクトの owner が member に追加:

```bash
# upstream 側
senko project members add --user-id <relay-bot-id> --role member
```

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

API キーは長命なので relay 再起動による定期更新は不要です。ただし API キーモードの upstream は試用用途なので、本番では OIDC + M2M + 定期リフレッシュを選んでください。

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

- relay 全般の運用 → [deploy.md](deploy.md)
- hook 実例 → [hooks.md](hooks.md)
- AI サンドボックスの end-to-end → [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)
