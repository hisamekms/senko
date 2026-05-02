# senko-web デプロイガイド

senko-web は TanStack Start (SSR) で動く web フロントエンド + BFF。OIDC 認証 (Auth.js) を web 側で終端し、認証後のリクエストを senko backend (`senko serve`) に中継する。

このディレクトリは **どのデプロイ先でも共通で必要になる事項** (env 変数 / tarball 入手 / 前提) と、デプロイ先別ガイドへの導線を集約する。

## v1 で対応するデプロイ先

| デプロイ先 | 状態 | ガイド |
| --- | --- | --- |
| AWS Lambda + Amazon Cognito | ✅ v1 対応 | [./aws-lambda-cognito.md](./aws-lambda-cognito.md) |
| Container (Docker) | 🚧 将来追加 | — |
| Vercel / Netlify | 🚧 将来追加 | — |
| Self-hosted Node (EC2 / VM) | 🚧 将来追加 | — |

## 全体マップ

```
Browser
  │
  ▼
senko-web (TanStack Start SSR + Auth.js BFF)
  │   ├─[OIDC]─► OIDC IdP (例: Amazon Cognito User Pool)
  │   │
  │   └─[Authorization: Bearer <ID/Access token>]
  ▼
senko backend (`senko serve`, OpenAPI)
```

- **senko-web**: SSR + 認証 BFF。tarball で配布される (`senko-web-${VERSION}.tar.gz`)
- **OIDC IdP**: ログインを担う。本ガイドの v1 サンプルは Amazon Cognito User Pool
- **senko backend**: 既にデプロイ済みの `senko serve` インスタンス。本ガイドの対象外 ([関連ドキュメント](#関連ドキュメント) を参照)

## 前提

- senko backend (`senko serve`) がデプロイ済みかつ HTTPS で到達可能 ([デプロイ手順](../server-remote/deploy.md), [AWS デプロイ例](../server-remote/aws-deployment.md))
- OIDC IdP (Cognito User Pool / Auth0 / Google など) を準備済み ([OIDC 認証ガイド](../server-remote/auth-oidc.md))
- senko-web tarball と senko backend のバージョン (= OpenAPI 仕様) が一致している。release ワークフローは `senko vX.Y.Z` タグで両者を同一 Release に co-publish するため、**同じ vX.Y.Z** を選べばよい

## env 変数 (web Lambda が要求するもの)

senko-web ランタイムが起動時に参照する環境変数の正典リスト。

| 変数名 | 必須 | 例 | 説明 |
| --- | --- | --- | --- |
| `SENKO_API_BASE_URL` | ✅ | `https://api.senko.example.com` | senko backend (`senko serve`) の HTTPS エンドポイント (API Gateway URL など) |
| `AUTH_SECRET` | ✅※ | `openssl rand -base64 32` の出力 | Auth.js session の署名・暗号化用シークレット (32 bytes 以上)。`AUTH_SECRET_ARN` を使う場合は不要 |
| `AUTH_URL` | ✅ | `https://app.senko.example.com/api/auth` | senko-web の公開 URL + `/api/auth` |
| `AUTH_OIDC_ISSUER` | ✅ | `https://cognito-idp.<region>.amazonaws.com/<user-pool-id>` | OIDC IdP の issuer URL |
| `AUTH_OIDC_CLIENT_ID` | ✅ | `(IdP の app client ID)` | OIDC アプリクライアント ID |
| `AUTH_OIDC_CLIENT_SECRET` | ✅※ | `(Secrets Manager 等から注入)` | OIDC アプリクライアントシークレット。`AUTH_OIDC_CLIENT_SECRET_ARN` を使う場合は不要 |
| `AUTH_SECRET_ARN` | — | `arn:aws:secretsmanager:<region>:<acct>:secret:<name>` | `AUTH_SECRET` の値を AWS Secrets Manager から runtime fetch する場合の ARN (※下記参照) |
| `AUTH_OIDC_CLIENT_SECRET_ARN` | — | `arn:aws:secretsmanager:<region>:<acct>:secret:<name>` | `AUTH_OIDC_CLIENT_SECRET` の値を AWS Secrets Manager から runtime fetch する場合の ARN (※下記参照) |

> `AUTH_URL` と `AUTH_OIDC_ISSUER` は **HTTPS スキーム必須**。`http://` を渡すと起動時に fail-fast する。
>
> ※ AUTH_SECRET / AUTH_OIDC_CLIENT_SECRET は **どちらか一方** (生値 or `_ARN`) を必ず本番で設定する。両方未設定だと起動時に fail-fast する。

### Secrets Manager ARN による runtime 解決 (AWS Lambda 向け)

`AUTH_SECRET_ARN` / `AUTH_OIDC_CLIENT_SECRET_ARN` を設定すると、senko-web は env の生値ではなく ARN を読んでリクエスト時に AWS Secrets Manager から `GetSecretValue` で取得する。値はプロセス内で **15 分間キャッシュ** され、同一 ARN への同時呼び出しは Promise レベルで dedupe される。

- **目的**: AWS Lambda の `lambda:GetFunctionConfiguration` 経由で env が読める範囲を狭める。env には ARN (機密ではない) のみが残る。
- **AWS SnapStart との整合性**: 解決はモジュール初期化時ではなくリクエスト時に行うため、SnapStart スナップショットに古い値が焼き込まれない。
- **優先順位**: `*_ARN` と生値の両方が設定された場合は **ARN が優先** され、初回解決時に `console.warn` が一度だけ出る。
- **後方互換**: `*_ARN` を未設定にしておけば、従来どおり生値の env がそのまま使われる (SDK 呼び出しなし)。
- **必要な IAM 権限**: Lambda 実行ロールに対象 ARN への `secretsmanager:GetSecretValue` を許可する (CDK では `secret.grantRead(fn)`)。
- **format 検証**: `*_ARN` は `arn:aws:secretsmanager:<region>:<account-id>:secret:<name>` 形式。production で形式が崩れていると起動時に fail-fast する。

## tarball の入手と検証

GitHub Releases に `senko-web-${VERSION}.tar.gz` と `senko-web-${VERSION}.tar.gz.sha256` が attach されている (senko vX.Y.Z タグの Release に同梱)。

```bash
# senko 本体と同じバージョンタグ (例)
SENKO_VERSION="0.42.0"
REPO="hisamekms/senko"
ASSET="senko-web-${SENKO_VERSION}.tar.gz"
BASE="https://github.com/${REPO}/releases/download/v${SENKO_VERSION}"

# ダウンロード
curl -fsSL -o "${ASSET}"        "${BASE}/${ASSET}"
curl -fsSL -o "${ASSET}.sha256" "${BASE}/${ASSET}.sha256"

# 検証 (Linux: GNU coreutils)
sha256sum -c "${ASSET}.sha256"
# 検証 (macOS)
# shasum -a 256 -c "${ASSET}.sha256"

# 展開
tar -xzf "${ASSET}"
# → ./senko-web-${SENKO_VERSION}/ ディレクトリができる
```

展開後のディレクトリレイアウト:

- `aws-lambda-handler.mjs` — AWS Lambda エントリポイント (`srvx/aws-lambda` の `toLambdaHandler` 経由で SSR fetch handler を委譲)
- `package.json` — `name` / `version` / `type=module` / `private` のみの最小メタデータ
- `dist/server/server-entry.js` — TanStack Start SSR build (`{ default: { fetch } }` を export)
- `dist/client/`, `dist/public/` — クライアント assets
- `node_modules/` — runtime 依存のみ (`npm ci --omit=dev` でステージング済)

サイズ目安は約 23 MiB (Contract で 50 MiB を上限としている)。

## デプロイ先別ガイド

- [AWS Lambda + Amazon Cognito](./aws-lambda-cognito.md) — v1 で公式サポート

他のデプロイ先 (container, Vercel/Netlify, 自前 Node) は将来追加予定。

## 関連ドキュメント

- senko backend のデプロイ: [デプロイガイド](../server-remote/deploy.md), [AWS デプロイ例](../server-remote/aws-deployment.md)
- OIDC 認証の仕様: [OIDC 認証ガイド](../server-remote/auth-oidc.md)
