# API キー認証

シンプルな Bearer トークン認証。

> **位置づけ**: API キー認証は **試用・初期動作確認** 用途に限定されます。本番運用では人間・bot ともに [OIDC 認証](auth-oidc.md) を推奨します (人間は OAuth Authorization Code + PKCE、bot は OAuth Client Credentials = M2M)。
>
> - `master_key` はどの認証モードでも **bootstrap 用** (ユーザ/API キーの初回発行) として常備する
> - IdP で Client Credentials を用意できない、あるいは JWT の短 TTL 運用を避けたい限定ケースでのみ、個別 API キーの継続利用を検討

## セットアップ

### 1. master key を生成

```bash
MASTER_KEY=$(openssl rand -base64 32)
export SENKO_AUTH_API_KEY_MASTER_KEY="$MASTER_KEY"
senko serve --host 0.0.0.0 --port 3142
```

もしくは config で:

```toml
[server.auth.api_key]
master_key = "..."
# または:
# master_key_arn = "arn:aws:secretsmanager:..."
```

**master key とは**: どの User にも紐づかない特権キー。ユーザ作成 (`POST /api/v1/users`) 等のブートストラップに使う。**通常の API 操作には使わない**こと。

### 2. master key でユーザを作る

```bash
curl -s -X POST https://senko.example.com/api/v1/users \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username":"alice"}' | jq .
# {"id": 2, "username": "alice", ...}
```

### 3. そのユーザ用の API キーを発行

```bash
curl -s -X POST https://senko.example.com/api/v1/users/2/api-keys \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"default"}' | jq .
# {"id": 3, "key": "sk_abc123...", "key_prefix": "sk_ab", ...}
```

`key` は **発行時の 1 回しか返りません**。失うと再発行するしかないので、安全な場所に保存。

### 4. クライアント側の設定

発行した API キーを渡します。

```bash
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="sk_abc123..."
senko task list
```

あるいは `.senko/config.local.toml` (git 管理外) に:

```toml
[cli.remote]
url = "https://senko.example.com"
token = "sk_abc123..."
```

## master key の管理

- **インターネットに出さない**。発行時も Secrets Manager 経由で注入
- **ローテーション**: `master_key_arn` を使っていれば Secrets Manager 側でローテート → サーバ再起動で反映
- **revoke 不可**: master key 自体には失効の仕組みなし。漏洩したら別の値に差し替えて再配布するしかない。通常の API キーと違い DB には保存されていない

## API キーの revoke

```bash
# ユーザ配下の API キー一覧
curl -s -H "Authorization: Bearer $MASTER_KEY" \
  https://senko.example.com/api/v1/users/2/api-keys | jq .

# 特定 key を削除
curl -s -X DELETE -H "Authorization: Bearer $MASTER_KEY" \
  https://senko.example.com/api/v1/users/2/api-keys/3
```

または `senko auth revoke <id>` (自分のキーのみ)。

## master key と通常 API キーの差

| | Master key | API key |
|---|---|---|
| User と紐付け | なし | あり |
| DB 保存 | されない (config/env のみ) | `api_keys` テーブルに hash で保存 |
| `POST /api/v1/users` 権限 | あり | **なし** |
| Project member 権限 | プロジェクト不問で通る | role に従う |
| revoke | 直接不可 (値の差し替え) | DB から削除 |

## 運用のコツ

- **デバイス別に発行**: 開発者は自分の端末ごとに別 API キーを作る (`name` に `"alice-laptop"` / `"alice-ci"` など)。紛失時に影響範囲を絞れる
- **master key は起動時のみの鍵として扱う**: 1 人目のユーザと最初の API キーを作ったら、以降 master key は使わない運用
- **漏洩対策**: ログに `Authorization: Bearer ...` が出力されないよう注意

## トラブルシューティング

| 症状 | 原因 | 対処 |
|---|---|---|
| 401 Unauthorized | token が無効 / 失効 | `senko auth status` や key 一覧で確認 |
| 403 Forbidden | master key 不要の操作に master key を使った、または member ではない | 通常 API キーを使う / member 追加 |
| 設定したのに `[server.auth.api_key]` が有効化されない | `master_key` / `master_key_arn` どちらも未設定、または env 名が typo | `senko config` で確認 |

## 次のステップ

- OIDC に乗り換え → [auth-oidc.md](auth-oidc.md)
- API Gateway 配下で使う → [auth-trusted-headers.md](auth-trusted-headers.md)
