# AWS Lambda + Amazon Cognito へのデプロイ

senko-web を AWS Lambda (SSR + Auth.js BFF) と Amazon Cognito User Pool で動かすための、AWS CDK (TypeScript) を使ったコピペ可能なデプロイ手順。

このページは [`./README.md`](./README.md) で示した「v1 で公式サポートするデプロイ先」の実装例です。tarball の入手・検証 / env 変数の正典リスト / 全体アーキテクチャは README を一次情報として参照してください。本ページは **AWS 個別の構築手順** に絞ります。

## このガイドで作るもの

- Web Lambda (Node 24 / ARM_64 / SnapStart 有効) — TanStack Start SSR + Auth.js BFF を 1 関数で提供
- HTTP API Gateway v2 — `$default` route で全リクエストを Lambda へ流す。**Cognito Authorizer は使わない** (Auth.js が cookie session で認証を担うため)
- Secrets Manager — `AUTH_SECRET` と `AUTH_OIDC_CLIENT_SECRET` を保管
- 既存 Cognito User Pool (Hosted UI domain 設定済み) との接続

```
Browser
  │
  ▼
[Web HTTP API GW]
  │
  ▼
[Web Lambda  (TanStack Start SSR + Auth.js BFF)]
  │   ├─[OIDC redirect]──► [Cognito Hosted UI / User Pool]
  │   │
  │   └─[Authorization: Bearer <token>]
  ▼
[Server HTTP API GW (= senko backend, 既設)]
  │
  ▼
[senko serve]
```

## 前提

- AWS アカウント、IAM ユーザー / ロール (Lambda / API GW v2 / Secrets Manager / IAM の作成権限)
- `aws` CLI がローカルで設定済み (`aws configure`)
- Node.js 24+ と AWS CDK v2 (`npm i -g aws-cdk`)
- 対象 AWS アカウント / リージョンで CDK bootstrap 済み (`cdk bootstrap aws://<acct>/<region>`)
- **既存の Cognito User Pool** + **Hosted UI domain (`*.auth.<region>.amazoncognito.com` または独自ドメイン)** + **app client**
  - Hosted UI domain が設定されていないと、issuer の OIDC discovery (`<issuer>/.well-known/openid-configuration`) が `authorization_endpoint` を返さず、Auth.js が起動時に失敗する
  - app client 設定: code grant flow を有効、OAuth scope `openid profile email`、Allowed callback URL に `https://<web-domain>/api/auth/callback/oidc`、refresh token expiration を任意 (デフォルト 30 日)
- senko backend (`senko serve`) がデプロイ済みで HTTPS エンドポイント (`SENKO_API_BASE_URL`) を持っている
- 公開ドメインの DNS / TLS — 独自ドメインを HTTP API GW に紐付けるか、GW 既定の `https://<api-id>.execute-api.<region>.amazonaws.com` をそのまま使う

## 手順

### 1. tarball を入手して展開

[`./README.md` の「tarball の入手と検証」](./README.md#tarball-の入手と検証) 手順を参照。展開後 `./senko-web-${SENKO_VERSION}/` が作られ、以下が並ぶ。

- `aws-lambda-handler.mjs` — Lambda エントリポイント (`srvx/aws-lambda` の `toLambdaHandler` 経由で SSR fetch handler を委譲)
- `package.json` (最小)
- `dist/server/server-entry.js` (TanStack Start SSR build)
- `dist/client/`, `dist/public/` (静的 assets)
- `node_modules/` (runtime 依存のみ)

CDK プロジェクト内では `cdk.out/senko-web/` 等に展開して `Code.fromAsset` で取り込みます (後述)。

### 2. CDK プロジェクトの雛形

```bash
mkdir senko-web-deploy && cd senko-web-deploy
cdk init app --language typescript
mkdir -p scripts
```

最終的な構成:

```
senko-web-deploy/
├── bin/app.ts
├── lib/senko-web-stack.ts
├── scripts/fetch-senko-web.sh
├── cdk.json
├── package.json
└── tsconfig.json
```

`cdk init` が生成した `bin/<project>.ts` は `bin/app.ts` にリネームし、`cdk.json` の `app` キーも `npx ts-node --prefer-ts-exts bin/app.ts` に揃えてください。

### 3. tarball 取得スクリプト (`scripts/fetch-senko-web.sh`)

`cdk synth` / `cdk deploy` の前に走らせて、tarball を `cdk.out/senko-web/` に展開する idempotent スクリプト。

```bash
#!/usr/bin/env bash
# scripts/fetch-senko-web.sh
set -euo pipefail

: "${SENKO_VERSION:?SENKO_VERSION is required (e.g. 0.42.0)}"
REPO="hisamekms/senko"
ASSET="senko-web-${SENKO_VERSION}.tar.gz"
BASE="https://github.com/${REPO}/releases/download/v${SENKO_VERSION}"

OUT_DIR="cdk.out/senko-web"
TARBALL_DIR="cdk.out/_tarball"
mkdir -p "${TARBALL_DIR}"

# ダウンロード (既にあれば skip)
if [ ! -f "${TARBALL_DIR}/${ASSET}" ]; then
  curl -fsSL -o "${TARBALL_DIR}/${ASSET}"        "${BASE}/${ASSET}"
  curl -fsSL -o "${TARBALL_DIR}/${ASSET}.sha256" "${BASE}/${ASSET}.sha256"
fi

# SHA256 検証
( cd "${TARBALL_DIR}" && sha256sum -c "${ASSET}.sha256" )

# 展開 (毎回クリーン)
rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
tar -xzf "${TARBALL_DIR}/${ASSET}" -C "${OUT_DIR}" --strip-components=1

echo "extracted: ${OUT_DIR}"
```

`package.json` に呼び出しを足しておくと楽:

```json
{
  "scripts": {
    "fetch": "bash scripts/fetch-senko-web.sh",
    "synth": "npm run fetch && cdk synth",
    "deploy": "npm run fetch && cdk deploy"
  }
}
```

### 4. CDK スタック (`lib/senko-web-stack.ts`)

```ts
import * as path from 'node:path'
import { CfnOutput, Duration, Stack, type StackProps } from 'aws-cdk-lib'
import { HttpApi } from 'aws-cdk-lib/aws-apigatewayv2'
import { HttpLambdaIntegration } from 'aws-cdk-lib/aws-apigatewayv2-integrations'
import {
  Alias,
  Architecture,
  type CfnFunction,
  Code,
  Function as LambdaFunction,
  Runtime,
} from 'aws-cdk-lib/aws-lambda'
import { Secret } from 'aws-cdk-lib/aws-secretsmanager'
import type { Construct } from 'constructs'

export interface SenkoWebStackProps extends StackProps {
  /** 公開 Web URL (末尾 `/` なし)。例: `https://app.senko.example.com` */
  readonly webPublicUrl: string
  /** senko backend の HTTPS エンドポイント (末尾 `/` なし) */
  readonly senkoApiBaseUrl: string
  /** Cognito User Pool の OIDC issuer URL */
  readonly oidcIssuer: string
  /** Cognito app client ID */
  readonly oidcClientId: string
  /** Secrets Manager に作成済みの AUTH_SECRET (32+ bytes) の ARN */
  readonly authSecretArn: string
  /** Secrets Manager に作成済みの OIDC client secret の ARN */
  readonly oidcClientSecretArn: string
}

export class SenkoWebStack extends Stack {
  constructor(scope: Construct, id: string, props: SenkoWebStackProps) {
    super(scope, id, props)

    const authSecret = Secret.fromSecretCompleteArn(
      this,
      'AuthSecret',
      props.authSecretArn,
    )
    const oidcSecret = Secret.fromSecretCompleteArn(
      this,
      'OidcClientSecret',
      props.oidcClientSecretArn,
    )

    const fn = new LambdaFunction(this, 'WebFunction', {
      runtime: Runtime.NODEJS_24_X,
      architecture: Architecture.ARM_64,
      handler: 'aws-lambda-handler.handler',
      code: Code.fromAsset(path.join(__dirname, '..', 'cdk.out', 'senko-web')),
      memorySize: 1024,
      timeout: Duration.seconds(15),
      environment: {
        SENKO_API_BASE_URL: props.senkoApiBaseUrl,
        AUTH_URL: `${props.webPublicUrl}/api/auth`,
        AUTH_OIDC_ISSUER: props.oidcIssuer,
        AUTH_OIDC_CLIENT_ID: props.oidcClientId,
        // ARN を渡すだけ — senko-web (>=0.43) はリクエスト時に
        // SecretsManager.GetSecretValue を呼んで値を取得し、プロセス内で
        // 15 分キャッシュする。env には ARN しか残らないため、
        // lambda:GetFunctionConfiguration で env を覗かれても秘密値は漏れない。
        AUTH_SECRET_ARN: props.authSecretArn,
        AUTH_OIDC_CLIENT_SECRET_ARN: props.oidcClientSecretArn,
      },
    })

    // SnapStart を CFN レベルで直接指定する。aws-cdk-lib 2.252.0 時点で
    // Runtime.NODEJS_24_X の supportsSnapStart フラグがまだ false のため、
    // 高レベル `snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS` を渡すと
    // CDK の validate に弾かれる (AWS Lambda 自体は Node 20+ で SnapStart
    // GA、Node 24 ランタイムも対応済み)。CDK 側が NODEJS_24_X の
    // supportsSnapStart を true にした時点で `addPropertyOverride` を
    // やめて `snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS` に戻して
    // よい (合成テンプレートは同一)。詳細は
    // `docs/knowledge/aws-cdk-snapstart-nodejs24.md` を参照。
    const cfnFn = fn.node.defaultChild as CfnFunction
    cfnFn.addPropertyOverride('SnapStart', { ApplyOn: 'PublishedVersions' })

    authSecret.grantRead(fn)
    oidcSecret.grantRead(fn)

    const liveAlias = new Alias(this, 'WebFunctionLive', {
      aliasName: 'live',
      version: fn.currentVersion,
    })

    const httpApi = new HttpApi(this, 'WebHttpApi', {
      defaultIntegration: new HttpLambdaIntegration(
        'DefaultIntegration',
        liveAlias,
      ),
    })

    new CfnOutput(this, 'WebApiEndpoint', {
      value: httpApi.apiEndpoint,
      description: 'Public endpoint of the senko-web HTTP API Gateway',
    })
  }
}
```

ポイント:

- `code: Code.fromAsset('cdk.out/senko-web')` で tarball 展開後ディレクトリをそのままアップロード。`aws-lambda-handler.mjs` と `dist/`、`node_modules/` が同梱されたまま Lambda に配置される。
- `handler: 'aws-lambda-handler.handler'` — tarball top-level の `aws-lambda-handler.mjs` がエクスポートする `handler` を呼ぶ。
- SnapStart は `addPropertyOverride('SnapStart', { ApplyOn: 'PublishedVersions' })` で直接 CFN プロパティとして指定し、`Alias` 経由で呼ぶ。alias を作らずに `$LATEST` を呼んでも SnapStart は効かない。aws-cdk-lib 2.252.0 の `Runtime.NODEJS_24_X` には `supportsSnapStart` フラグが立っていないため、高レベルの `snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS` を渡すと synth が落ちる (合成された CFN テンプレートはどちらでも同一)。CDK 側で `supportsSnapStart` が立った時点で override を外せる — 観測状況は `docs/knowledge/aws-cdk-snapstart-nodejs24.md` を参照。
- HTTP API GW は **Cognito Authorizer を付けない**。OIDC ログインは Auth.js が `/api/auth/*` で完結する。

### 5. `bin/app.ts`

```ts
#!/usr/bin/env node
import 'source-map-support/register'
import { App } from 'aws-cdk-lib'
import { SenkoWebStack } from '../lib/senko-web-stack'

const app = new App()

new SenkoWebStack(app, 'SenkoWebStack', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
  webPublicUrl: app.node.tryGetContext('webPublicUrl'),
  senkoApiBaseUrl: app.node.tryGetContext('senkoApiBaseUrl'),
  oidcIssuer: app.node.tryGetContext('oidcIssuer'),
  oidcClientId: app.node.tryGetContext('oidcClientId'),
  authSecretArn: app.node.tryGetContext('authSecretArn'),
  oidcClientSecretArn: app.node.tryGetContext('oidcClientSecretArn'),
})
```

値は `cdk.json` の `context` か `cdk deploy -c key=value` で渡します。

### 6. Cognito redirect URI を登録

```bash
USER_POOL_ID="ap-northeast-1_XXXXXXXXX"
CLIENT_ID="xxxxxxxxxxxxxxxxxxxxxxxxxx"
WEB_DOMAIN="app.senko.example.com"

aws cognito-idp update-user-pool-client \
  --user-pool-id "${USER_POOL_ID}" \
  --client-id "${CLIENT_ID}" \
  --callback-urls "https://${WEB_DOMAIN}/api/auth/callback/oidc" \
  --logout-urls   "https://${WEB_DOMAIN}/" \
  --allowed-o-auth-flows code \
  --allowed-o-auth-scopes openid profile email \
  --allowed-o-auth-flows-user-pool-client \
  --supported-identity-providers COGNITO
```

callback の path は `/api/auth/callback/oidc` 固定 (Auth.js の OIDC provider id が `oidc` のため)。

### 7. Secrets Manager に AUTH_SECRET と OIDC client secret を作成

```bash
# Auth.js の session 署名・暗号化シークレット (32+ bytes)
aws secretsmanager create-secret \
  --name senko-web/auth-secret \
  --secret-string "$(openssl rand -base64 32)"

# Cognito app client の client secret (コンソールまたは aws cli で取得)
COGNITO_CLIENT_SECRET=$(aws cognito-idp describe-user-pool-client \
  --user-pool-id "${USER_POOL_ID}" \
  --client-id "${CLIENT_ID}" \
  --query 'UserPoolClient.ClientSecret' \
  --output text)

aws secretsmanager create-secret \
  --name senko-web/oidc-client-secret \
  --secret-string "${COGNITO_CLIENT_SECRET}"
```

作成された ARN をメモしておきます (`aws secretsmanager describe-secret --secret-id ... --query ARN --output text`)。

### 8. デプロイ

`SENKO_VERSION` を指定して tarball を取得 → CDK で deploy。

```bash
export SENKO_VERSION="0.42.0"

npm run deploy -- \
  -c webPublicUrl=https://app.senko.example.com \
  -c senkoApiBaseUrl=https://api.senko.example.com \
  -c oidcIssuer=https://cognito-idp.ap-northeast-1.amazonaws.com/ap-northeast-1_XXXXXXXXX \
  -c oidcClientId=xxxxxxxxxxxxxxxxxxxxxxxxxx \
  -c authSecretArn=arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko-web/auth-secret-AbCdEf \
  -c oidcClientSecretArn=arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko-web/oidc-client-secret-GhIjKl
```

deploy 出力の `WebApiEndpoint` が HTTP API GW の URL です。独自ドメインを使わない場合、この URL を `webPublicUrl` として再 deploy するか、API GW の Custom domain を別 Stack で被せて DNS を切り替えます。

### 9. 動作確認

```bash
WEB_URL="https://app.senko.example.com"

# 1. 未ログインのセッション取得 (200 + body は null)
curl -i "${WEB_URL}/api/auth/session"

# 2. ブラウザで以下を開く → Cognito Hosted UI へリダイレクト → 戻ってきて
#    /api/auth/session が user 情報を返すようになる
#    ${WEB_URL}/api/auth/signin/oidc

# 3. ブラウザで取得した cookie を保存して BFF 経由 backend を叩く
#    (cookie.txt は Chrome devtools 等から書き出す)
curl -b cookie.txt -i "${WEB_URL}/api/senko/api/v1/projects"
# → 200 (cookie 有効) または 401 (未ログイン / 有効期限切れ)
```

## env 変数 ↔ CDK 対応表

env 変数の正典リスト (必須 / 例 / 説明) は [README の env 表](./README.md#env-変数-web-lambda-が要求するもの) を参照。下表は **どの env が CDK 上のどの値から来るか** だけを示します。

| env 変数 | CDK 上の出処 | 備考 |
| --- | --- | --- |
| `SENKO_API_BASE_URL` | `props.senkoApiBaseUrl` (リテラル) | 末尾 `/` なし |
| `AUTH_URL` | `` `${props.webPublicUrl}/api/auth` `` | HTTPS 必須 |
| `AUTH_OIDC_ISSUER` | `props.oidcIssuer` (リテラル) | HTTPS 必須。例: `https://cognito-idp.<region>.amazonaws.com/<user-pool-id>` |
| `AUTH_OIDC_CLIENT_ID` | `props.oidcClientId` (リテラル) | secret ではない |
| `AUTH_OIDC_CLIENT_SECRET_ARN` | `props.oidcClientSecretArn` (リテラル ARN) | senko-web が runtime に SecretsManager から fetch |
| `AUTH_SECRET_ARN` | `props.authSecretArn` (リテラル ARN) | senko-web が runtime に SecretsManager から fetch |

env に渡るのは ARN 文字列のみ (機密ではない)。実際の `AUTH_SECRET` / `AUTH_OIDC_CLIENT_SECRET` 値は Lambda 実行中に `SecretsManagerClient.send(GetSecretValueCommand)` で取得され、プロセス内で 15 分キャッシュされる。`secret.grantRead(fn)` によって Lambda 実行ロールに `secretsmanager:GetSecretValue` が付与されるので追加 IAM 設定は不要。

> **以前のバージョン (`<= 0.42`) との違い**: 0.42 までは `AUTH_SECRET` / `AUTH_OIDC_CLIENT_SECRET` を `secretValue.unsafeUnwrap()` で CFN dynamic reference 経由 env に渡していたため、deploy 後の env には生値が残っていた。`lambda:GetFunctionConfiguration` 権限を持つ IAM principal が env を読むと秘密値が漏れるリスクがあった。`AUTH_SECRET_ARN` 形式に切り替えると env には ARN しか残らないため、この経路の漏洩リスクを排除できる。0.42 以前と互換のため、生値の env (`AUTH_SECRET` / `AUTH_OIDC_CLIENT_SECRET`) を渡す形は引き続きサポートされる。

## よくある失敗とその対処

- **`redirect_uri_mismatch`** — Cognito 側で登録した callback URL と Auth.js が要求する URL が完全一致していません (`scheme + host + port + path` まで)。Auth.js の OIDC provider id は `oidc` 固定なので、callback path は **必ず** `/api/auth/callback/oidc` になります。
- **Lambda 起動時に `AUTH_SECRET is required in production` で fail-fast** — `AUTH_SECRET` / `AUTH_SECRET_ARN` のどちらも未設定。`aws lambda get-function-configuration --function-name <fn>` で env が渡っているか確認。
- **`AUTH_SECRET_ARN does not look like a Secrets Manager ARN` で fail-fast** — ARN 文字列が typo (例: `arn:aws:s3:...`)。期待形式は `arn:aws:secretsmanager:<region>:<account-id>:secret:<name>`。
- **request 処理中に `AccessDeniedException` / `ResourceNotFoundException`** — Lambda 実行ロールに `secretsmanager:GetSecretValue` がない、または ARN が指す Secret が別アカウント / 別 region。CDK の `secret.grantRead(fn)` を実行ロールに反映 (`cdk deploy` 後に IAM role の inline policy を確認)。
- **`AUTH_URL must be an HTTPS URL` / `AUTH_OIDC_ISSUER must be an HTTPS URL`** — Task #406 で追加された起動時 fail-fast。HTTP API GW の `execute-api` ドメインは HTTPS なので素直に渡せば通ります。
- **`Set-Cookie` ヘッダが大きすぎる / セッションが立たない** — Cognito の ID/Access token に大量の claim が乗ると Auth.js が cookie を分割しても収まりません。Cognito 側で custom scope / 渡す claim を絞ってください。
- **SnapStart の効果が出ない (cold start のまま)** — Alias を作らず `$LATEST` を呼んでいないか確認。SnapStart は published version 単位でスナップショットされるため、必ず alias 経由で呼びます。
- **SnapStart で復元後だけ動作がおかしい** — top-level (INIT 段階) で乱数生成 / DB コネクション保持 / 時刻スナップショット系をしていると restore 後に壊れます。Auth.js + srvx 自体は INIT で外部ネットワークコールを持ちませんが、将来 INIT に重い初期化を足すときは [AWS の SnapStart 互換性ガイド](https://docs.aws.amazon.com/lambda/latest/dg/snapstart-uniqueness.html) を確認してください。
- **CSP nonce が効かない / ブラウザに 304 が返る** — senko-web の SSR は per-request nonce を script タグに付けるため、HTML レスポンスは絶対にキャッシュしてはいけません。CloudFront を前段に置く場合は origin response policy で `text/html` を `Cache-Control: private, no-store` にしてください。

## 他の OIDC IdP に切り替える場合

`AUTH_OIDC_ISSUER` / `AUTH_OIDC_CLIENT_ID` / `AUTH_OIDC_CLIENT_SECRET` の 3 つを差し替えれば、Keycloak / Auth0 / Google Workspace 等にそのまま切り替えられます。HTTP API GW は Cognito Authorizer を使っていないため、CDK スタックに変更は不要です。

- 各 IdP 側の callback URL 登録は同じ path (`/api/auth/callback/oidc`)
- IdP 側で `openid profile email` を含む scope と Authorization Code Flow + PKCE が許可されていること
- discovery エンドポイント (`<issuer>/.well-known/openid-configuration`) が `authorization_endpoint` / `token_endpoint` / `jwks_uri` を返すこと

Cognito 固有の手順 (Hosted UI domain / `aws cognito-idp update-user-pool-client`) はこのとき不要です。

## 任意: WAF / CloudFront を前段に足す

本番運用ではレート制限や IP 制限のために WAF / CloudFront を被せたくなることがあります。骨子のみ示します。

```ts
// WAF Web ACL を HTTP API GW v2 に attach する例
import { CfnWebACL, CfnWebACLAssociation } from 'aws-cdk-lib/aws-wafv2'

const acl = new CfnWebACL(this, 'WebAcl', {
  scope: 'REGIONAL',
  defaultAction: { allow: {} },
  visibilityConfig: {
    cloudWatchMetricsEnabled: true,
    metricName: 'senko-web-acl',
    sampledRequestsEnabled: true,
  },
  rules: [/* AWSManagedRulesCommonRuleSet など */],
})

new CfnWebACLAssociation(this, 'WebAclAssoc', {
  resourceArn: `arn:aws:apigateway:${this.region}::/apis/${httpApi.apiId}/stages/$default`,
  webAclArn: acl.attrArn,
})
```

CloudFront を被せる場合は **`text/html` をキャッシュしない** ことだけ注意 (CSP nonce が壊れる)。`/_build/*` 配下の静的 assets は逆にアグレッシブにキャッシュして問題ありません。

## 関連ドキュメント

- [senko-web デプロイガイド (README)](./README.md) — env 変数 / tarball 入手 / 全体マップ
- [senko backend デプロイ](../server-remote/deploy.md), [AWS デプロイ例](../server-remote/aws-deployment.md)
- [OIDC 認証ガイド](../server-remote/auth-oidc.md)
