# relay モードで `senko serve` をデプロイする

Relay サーバは DB を持たず、上流の direct サーバへ HTTP 転送するだけの薄いサーバ。relay として起動する専用フラグは無く、`[server.relay] url` (env: `SENKO_SERVER_RELAY_URL`) が設定された状態で `senko serve` を起動すると自動的に relay モードに入ります。

> **重要な前提: relay は inbound 認証をしない**
>
> relay モードの `senko serve` は内部で `auth_mode: None` 固定で起動し、`[server.auth.*]` の設定は **読み込まれず無視** されます。relay に届いたリクエストは認証検査をスキップして upstream に転送されます。したがって relay は **閉鎖ネットワーク (sandbox-only network / VPN 内 / loopback 等) 前提** で運用し、到達可能範囲を限定することで実質的な認可を与える設計です。
>
> 公開ネットワーク上で relay を動かしてはいけません。認可は reverse proxy (nginx の IP allowlist / mTLS / 別の API Gateway) を relay の前段に挟む形で外付けにするしかありません。

使い所の判断: [Runtime の使い分け](../../explanation/runtimes.md)

## 典型ユースケース

1. **AI サンドボックスからの upstream アクセス集約**
   - sandbox からは egress が制限され、relay 以外に外向き通信できない
   - relay が upstream 向けの credential (session API キー or M2M JWT) を預かって転送
2. **閉鎖ネットワーク内のクライアント → 外部 upstream**
   - 社内から外部 SaaS 的に動く senko サーバへの中継点
   - relay 側で監査ログ / egress 統制を集約
3. **上流認証の単点集約**
   - 複数の小さなクライアントが個別に credential を持つのを避け、relay に任せる
   - ただし relay 自体は認証しないので、前段のネットワーク境界 or 外部プロキシで入口を守ること

逆に **向かないユースケース**:

- 公開インターネットからの直接受け口 (認証機構がないので即侵入される)
- マルチテナントの認証分離 (relay 1 台では inbound のテナント区別ができない)

## 最小構成

```bash
# 上流サーバと認証用 token (= 上流で受理される credential)
export SENKO_SERVER_RELAY_URL="https://senko-upstream.example.com"
export SENKO_SERVER_RELAY_TOKEN="<upstream へ送る Bearer 値>"

# relay を起動 (閉鎖ネットワーク内で listen)
#   SENKO_SERVER_RELAY_URL が設定されているので relay モードで起動する
senko serve --host 127.0.0.1 --port 3142
```

config ファイル版:

```toml
[server]
host = "127.0.0.1"     # 閉鎖ネットワーク内からのみ到達可
port = 3142

[server.relay]
url   = "https://senko-upstream.example.com"
token = "<upstream へ送る Bearer 値>"
```

> `[server.auth.*]` / `[backend.*]` を書いても proxy mode では無視されます。書いても起動エラーにはなりませんが、効かないので書かない方が混乱しません。

## 挙動

relay は受け取った HTTP リクエストを以下のように処理します:

1. **認証チェックなし** — auth_mode が None なので無条件で通す
2. upstream へ転送する際の Authorization ヘッダを決定:
   - `[server.relay] token` が設定されていれば → **この token に差し替え** (substitution mode)
   - 未設定なら → **クライアントの Authorization ヘッダをそのまま透過** (passthrough mode)
3. upstream のレスポンスをそのまま返す
4. 該当 action の `[server.relay.<action>.hooks.<name>]` が発火 (upstream 呼び出し成功後)

## substitution モード (token 設定)

```toml
[server.relay]
url   = "https://senko-upstream.example.com"
token = "<upstream で受理される Bearer 値>"
```

`token` に入れる値の選択肢は upstream の認証モード次第:

- upstream が OIDC: **senko が発行した session API キー** (`senko auth login` + `senko auth token` で取得、TTL は `[server.auth.oidc.session]` に従う) または **IdP から直接取得した M2M JWT** (IdP の access_token_lifetime で失効)
- upstream が trusted_headers: **API Gateway が受理する JWT** (IdP の発行する access_token をそのまま)
- upstream が API キー: **master_key で発行した通常 API キー** (長命、試用用途)

挙動:

- **クライアントの credential は upstream に届かない** (relay が受け取って捨てる)
- 上流から見ると「relay が 1 identity の代表としてすべてのリクエストを送っている」
- 上流ログには個別クライアント情報が残らないので、**relay 側で監査ログを取る**必要あり ([`[server.relay.*]` hook の実例](hooks.md))

token の選び方・取得手順・更新パターンは [トークン中継 (Token Relay) パターン](token-relay.md) を参照。

## passthrough モード (token 未設定)

```toml
[server.relay]
url = "https://senko-upstream.example.com"
# token を書かない
```

- relay は Authorization ヘッダに触れず **そのまま透過**
- 上流で `[server.auth.oidc]` 等を有効化しておけば、クライアントの JWT / API キーを上流で検証する構成

**注意**: このモードでも relay 自体は無認証なので、クライアントが何も credential を送らなかった場合、relay は空の Authorization ヘッダを上流に送り、上流で 401 で弾かれます (relay は 401 を返すわけではなく上流の 401 を透過するだけ)。

## ヘルスチェック

```
GET /api/v1/health
```

認証不要・上流を叩かず即 200。Load Balancer から使えます。

## 運用 Tips

- **relay はステートレス**: 複数インスタンスで簡単にスケールアウト可。ただし hook で集計する場合は各インスタンスが独立して吐くことに注意
- **上流との TLS は証明書検証を省かない**
- **ネットワーク境界の確認**: ファイアウォール / network namespace / compose network で relay 到達経路を限定。公開にならない設計を起動前に検証
- **relay の hook は audit 専用**に使う (重処理は upstream 側か外部システムへ)
- **上流への token が短命** (IdP 発行の M2M JWT など) の場合は定期更新が必要 — [トークン中継 (Token Relay) パターン](token-relay.md)

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| 502 Bad Gateway | 上流が落ちている / ネットワーク不通 / relay → upstream の DNS 失敗 |
| 401 が返る (substitution モード) | `[server.relay] token` の値が上流で受理されない。失効した JWT / 権限不足の API キー / audience mismatch など |
| 401 が返る (passthrough モード) | クライアントが credential を送っていない or 上流で弱い credential が弾かれている |
| hook が発火しない | `[server.relay.*]` に書くべき hook を誤って `[server.remote.*]` や `[cli.*]` に書いていないか / runtime warn を確認 |
| クライアントから relay に認証なしで繋げてしまう | これは仕様。**閉鎖ネットワークで運用すること**。公開したい場合は reverse proxy / API Gateway を前段に挟んで外部で認可する |

## 次のステップ

- token の選び方と更新運用 → [トークン中継 (Token Relay) パターン](token-relay.md)
- hook 実例 → [`[server.relay.*]` hook の実例](hooks.md)
- AI サンドボックス構成の end-to-end → [CLI → Relay → Remote → PostgreSQL](../../getting-started/cli-relay-remote-postgres.md)
