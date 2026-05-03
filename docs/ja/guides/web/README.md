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
| `SENKO_AUTH_REQUIRED_SCOPE` | — | `senko:access` | 任意。設定すると、OAuth `access_token` の `scope` claim (空白区切り) にこの値が含まれない sign-in を reject する |
| `SENKO_AUTH_REQUIRED_GROUPS` | — | `senko` または `senko,senko-admin` | 任意。カンマ区切り allow-list。`SENKO_AUTH_GROUPS_CLAIM` で指定した access_token claim と ANY 一致しない sign-in を reject する |
| `SENKO_AUTH_GROUPS_CLAIM` | — | `senko_groups` / `cognito:groups` / `groups` | 任意。groups を保持する claim 名。**`SENKO_AUTH_REQUIRED_GROUPS` を設定するなら必須** — 未設定だと起動時に warn ログが出て全 sign-in が reject される (fail-secure)。claim 値は JSON array / カンマ区切り / 空白区切りのいずれにも対応 |

> `AUTH_URL` と `AUTH_OIDC_ISSUER` は **HTTPS スキーム必須**。`http://` を渡すと起動時に fail-fast する。
>
> ※ AUTH_SECRET / AUTH_OIDC_CLIENT_SECRET は **どちらか一方** (生値 or `_ARN`) を必ず本番で設定する。両方未設定だと起動時に fail-fast する。

> `SENKO_AUTH_*` の 3 行は **任意の sign-in gate**。3 つとも未設定 (default) なら IdP に通った全ユーザーが sign-in できる。設定した場合は access_token を decode (署名検証は IdP 済前提でスキップ) して、設定された scope / group claim を満たさない sign-in を reject する。Cognito の pre-token Lambda レシピは [./aws-lambda-cognito.md#cognito-グループによる-sign-in-制限](./aws-lambda-cognito.md#cognito-グループによる-sign-in-制限) を参照。

### Secrets Manager ARN による runtime 解決 (AWS Lambda 向け)

`AUTH_SECRET_ARN` / `AUTH_OIDC_CLIENT_SECRET_ARN` を設定すると、senko-web は env の生値ではなく ARN を読んでリクエスト時に AWS Secrets Manager から `GetSecretValue` で取得する。値はプロセス内で **15 分間キャッシュ** され、同一 ARN への同時呼び出しは Promise レベルで dedupe される。

- **目的**: AWS Lambda の `lambda:GetFunctionConfiguration` 経由で env が読める範囲を狭める。env には ARN (機密ではない) のみが残る。
- **AWS SnapStart との整合性**: 解決はモジュール初期化時ではなくリクエスト時に行うため、SnapStart スナップショットに古い値が焼き込まれない。
- **優先順位**: `*_ARN` と生値の両方が設定された場合は **ARN が優先** され、初回解決時に `console.warn` が一度だけ出る。
- **後方互換**: `*_ARN` を未設定にしておけば、従来どおり生値の env がそのまま使われる (SDK 呼び出しなし)。
- **必要な IAM 権限**: Lambda 実行ロールに対象 ARN への `secretsmanager:GetSecretValue` を許可する (CDK では `secret.grantRead(fn)`)。
- **format 検証**: `*_ARN` は `arn:aws:secretsmanager:<region>:<account-id>:secret:<name>` 形式。production で形式が崩れていると起動時に fail-fast する。

## セキュリティヘッダ env vars (任意)

senko-web ランタイムは Lambda で `Content-Security-Policy` / `Strict-Transport-Security` / `Permissions-Policy` / `Cross-Origin-Opener-Policy` / `Cross-Origin-Resource-Policy` などのセキュリティヘッダを既定で発行する。下記の env で挙動を切り替えられる (全て省略可、デフォルトは現状のセキュアな値)。

| 変数名 | 効果 | 例 |
| --- | --- | --- |
| `CSP_REPORT_ONLY` | `true` で本番でも `Content-Security-Policy-Report-Only` を発行 (enforcing CSP は出さない)。CSP 変更の段階的 enforce 用 | `true` |
| `CSP_REPORT_URI` | CSP に `report-uri <url>` を追加 (違反観測用) | `https://reports.example.com/csp` |
| `CSP_EXTRA_CONNECT_SRC` | `connect-src` に追加する origin (カンマ or 空白区切り) | `https://api.example.com, https://logs.example.com` |
| `CSP_EXTRA_IMG_SRC` | `img-src` に追加する origin | `https://images.example.com` |
| `CSP_EXTRA_SCRIPT_SRC` | `script-src` に追加する origin | `https://cdn.example.com` |
| `CSP_EXTRA_STYLE_SRC` | `style-src` に追加する origin | `https://fonts.googleapis.com` |
| `CSP_EXTRA_FONT_SRC` | `font-src` に追加する origin | `https://fonts.gstatic.com` |
| `HSTS_DISABLED` | `true` で Lambda は `Strict-Transport-Security` を一切出さない (CloudFront 等の前段に任せる用) | `true` |
| `HSTS_MAX_AGE` | HSTS の `max-age` を秒単位で上書き (デフォルト `31536000` = 1年) | `63072000` |
| `HSTS_PRELOAD` | `true` で HSTS に `; preload` を付与 (HSTS preload list 申請時) | `true` |
| `COOP_DISABLED` | `true` で `Cross-Origin-Opener-Policy` を出さない | `true` |
| `CORP_DISABLED` | `true` で `Cross-Origin-Resource-Policy` を出さない | `true` |

> ヘッダ値の inject 攻撃を防ぐため、`CSP_EXTRA_*` の各 token と `CSP_REPORT_URI` からは `;` `\r` `\n` および内部の空白文字を strip する。proxy/CDN 経由で注入された不正値があっても CSP に他の directive を混入させることはできない。

### 運用例 1: CloudFront との二重設定回避

CloudFront の Response Headers Policy 等で HSTS や Permissions-Policy を一括上書きしている場合、Lambda 側からも同一ヘッダを送ると CloudFront 側が優先されるか衝突する可能性がある。Lambda 側を明示的に off にすると意図が明確になる。

```bash
# CloudFront 側で HSTS を発行する場合
HSTS_DISABLED=true

# CloudFront 側で COOP/CORP を上書きする場合
COOP_DISABLED=true
CORP_DISABLED=true
```

### 運用例 2: CSP の段階的 enforce (staged rollout)

新しい CSP ポリシーを enforce 前に違反観測したい場合、まず Report-Only モードで発行して `report-uri` でテレメトリを集め、問題がなければ flag を外して enforce に昇格させる。

```bash
# Step 1: 候補ポリシーを Report-Only で観測
CSP_REPORT_ONLY=true
CSP_REPORT_URI=https://reports.example.com/csp

# Step 2: テレメトリ確認後、CSP_REPORT_ONLY を消して enforce 昇格
# (CSP_REPORT_URI は残しておけば enforcing CSP の違反も収集できる)
```

### 運用例 3: 外部 origin の追加 (CDN / アナリティクス等)

senko-web は既定で `'self'` のみを許可する。外部 CDN や analytics SDK を追加する場合は CSP_EXTRA_* を使う。

```bash
CSP_EXTRA_SCRIPT_SRC=https://cdn.example.com
CSP_EXTRA_CONNECT_SRC=https://api.example.com, https://logs.example.com
CSP_EXTRA_IMG_SRC=https://images.example.com
```

### 運用例 4: HSTS preload list 申請

Chromium の HSTS preload list 申請には `max-age >= 31536000` (1年) と `; preload` が必須。実運用では 2 年程度を推奨。

```bash
HSTS_MAX_AGE=63072000
HSTS_PRELOAD=true
```

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
