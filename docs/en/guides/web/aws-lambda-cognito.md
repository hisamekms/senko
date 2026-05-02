# Deploying to AWS Lambda + Amazon Cognito

A copy-pasteable AWS CDK (TypeScript) recipe for running senko-web (SSR + Auth.js BFF) on AWS Lambda backed by an Amazon Cognito User Pool.

This page is the implementation example for the "officially supported v1 deployment target" listed in [`./README.md`](./README.md). For the canonical env var list, tarball acquisition, and overall architecture, treat the README as the source of truth — this page focuses on **the AWS-specific build steps**.

## What this guide builds

- A Web Lambda (Node 24 / ARM_64 / SnapStart enabled) hosting both the TanStack Start SSR app and the Auth.js BFF in one function.
- An HTTP API Gateway v2 with a `$default` route forwarding everything to the Lambda. **No Cognito Authorizer** — Auth.js handles the session via cookies.
- Secrets Manager for `AUTH_SECRET` and `AUTH_OIDC_CLIENT_SECRET`.
- A connection to your existing Cognito User Pool (with a Hosted UI domain configured).

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
[Server HTTP API GW (= senko backend, pre-existing)]
  │
  ▼
[senko serve]
```

## Prerequisites

- An AWS account and an IAM principal with permission to create Lambda / API GW v2 / Secrets Manager / IAM resources.
- `aws` CLI configured locally (`aws configure`).
- Node.js 24+ and AWS CDK v2 (`npm i -g aws-cdk`).
- CDK bootstrapped in the target account/region (`cdk bootstrap aws://<acct>/<region>`).
- **An existing Cognito User Pool** + **Hosted UI domain (`*.auth.<region>.amazoncognito.com` or a custom domain)** + **app client**:
  - Without a Hosted UI domain, the issuer's OIDC discovery document (`<issuer>/.well-known/openid-configuration`) does not publish an `authorization_endpoint` and Auth.js fails at startup.
  - App client config: enable the code grant flow, OAuth scopes `openid profile email`, allowed callback URL `https://<web-domain>/api/auth/callback/oidc`, refresh-token expiration as you like (default 30 days).
- senko backend (`senko serve`) already deployed and exposing an HTTPS endpoint (`SENKO_API_BASE_URL`).
- DNS / TLS for the public domain — either bring a custom domain to HTTP API GW or accept the GW-issued URL `https://<api-id>.execute-api.<region>.amazonaws.com`.

## Steps

### 1. Download and extract the tarball

Follow the [README's "Downloading and Verifying the Tarball" section](./README.md#downloading-and-verifying-the-tarball). After extraction, `./senko-web-${SENKO_VERSION}/` contains:

- `aws-lambda-handler.mjs` — Lambda entry point (delegates the SSR fetch handler via `srvx/aws-lambda`'s `toLambdaHandler`)
- `package.json` (minimal)
- `dist/server/server-entry.js` (TanStack Start SSR build)
- `dist/client/`, `dist/public/` (static assets)
- `node_modules/` (runtime deps only)

Inside the CDK project, extract into `cdk.out/senko-web/` and pull it in via `Code.fromAsset` (shown below).

### 2. CDK project layout

```bash
mkdir senko-web-deploy && cd senko-web-deploy
cdk init app --language typescript
mkdir -p scripts
```

Final layout:

```
senko-web-deploy/
├── bin/app.ts
├── lib/senko-web-stack.ts
├── scripts/fetch-senko-web.sh
├── cdk.json
├── package.json
└── tsconfig.json
```

Rename the `cdk init`-generated `bin/<project>.ts` to `bin/app.ts` and update the `app` key in `cdk.json` to `npx ts-node --prefer-ts-exts bin/app.ts`.

### 3. Tarball-fetch script (`scripts/fetch-senko-web.sh`)

An idempotent script that runs before `cdk synth` / `cdk deploy` and stages the tarball into `cdk.out/senko-web/`.

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

# Download (skip if already present)
if [ ! -f "${TARBALL_DIR}/${ASSET}" ]; then
  curl -fsSL -o "${TARBALL_DIR}/${ASSET}"        "${BASE}/${ASSET}"
  curl -fsSL -o "${TARBALL_DIR}/${ASSET}.sha256" "${BASE}/${ASSET}.sha256"
fi

# Verify SHA256
( cd "${TARBALL_DIR}" && sha256sum -c "${ASSET}.sha256" )

# Extract (clean each time)
rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
tar -xzf "${TARBALL_DIR}/${ASSET}" -C "${OUT_DIR}" --strip-components=1

echo "extracted: ${OUT_DIR}"
```

Adding npm scripts makes invocation easy:

```json
{
  "scripts": {
    "fetch": "bash scripts/fetch-senko-web.sh",
    "synth": "npm run fetch && cdk synth",
    "deploy": "npm run fetch && cdk deploy"
  }
}
```

### 4. CDK stack (`lib/senko-web-stack.ts`)

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
  /** Public web URL (no trailing `/`). Example: `https://app.senko.example.com` */
  readonly webPublicUrl: string
  /** HTTPS endpoint of the senko backend (no trailing `/`) */
  readonly senkoApiBaseUrl: string
  /** OIDC issuer URL of the Cognito User Pool */
  readonly oidcIssuer: string
  /** Cognito app client ID */
  readonly oidcClientId: string
  /** ARN of the AUTH_SECRET (32+ bytes) already stored in Secrets Manager */
  readonly authSecretArn: string
  /** ARN of the OIDC client secret already stored in Secrets Manager */
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
        // unsafeUnwrap() expands to a CloudFormation
        // `{{resolve:secretsmanager:arn}}` dynamic reference; the literal
        // value never appears in the synthesized template.
        AUTH_SECRET: authSecret.secretValue.unsafeUnwrap(),
        AUTH_OIDC_CLIENT_SECRET: oidcSecret.secretValue.unsafeUnwrap(),
      },
    })

    // Apply SnapStart at the CFN level. As of aws-cdk-lib 2.252.0,
    // Runtime.NODEJS_24_X still has supportsSnapStart=false, so passing
    // the high-level `snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS`
    // prop fails CDK validation (AWS Lambda itself supports SnapStart on
    // Node 20+ — including the Node 24 runtime). Once the CDK runtime
    // metadata flips `supportsSnapStart` to true for NODEJS_24_X, the
    // override below can be replaced with the regular `snapStart` prop
    // without changing the synthesized template. See
    // `docs/knowledge/aws-cdk-snapstart-nodejs24.md` for the observed
    // status and the removal trigger.
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

Key points:

- `code: Code.fromAsset('cdk.out/senko-web')` uploads the extracted tarball directory as-is. `aws-lambda-handler.mjs`, `dist/`, and `node_modules/` end up alongside each other in the Lambda package.
- `handler: 'aws-lambda-handler.handler'` — invoke the `handler` exported by the top-level `aws-lambda-handler.mjs` from the tarball.
- SnapStart is set with `addPropertyOverride('SnapStart', { ApplyOn: 'PublishedVersions' })` and invocations are routed through an `Alias`; calling `$LATEST` directly bypasses SnapStart. The `Runtime.NODEJS_24_X` constant in aws-cdk-lib 2.252.0 does not yet expose a `supportsSnapStart` flag, so the high-level `snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS` prop fails synth even though the synthesized CFN template would be identical. The override can be retired once CDK flips `supportsSnapStart` to true for `NODEJS_24_X` — see `docs/knowledge/aws-cdk-snapstart-nodejs24.md` for the current observation.
- The HTTP API GW has **no Cognito Authorizer**. OIDC login is fully handled inside Auth.js under `/api/auth/*`.

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

Pass values via `cdk.json`'s `context` block or `cdk deploy -c key=value`.

### 6. Register the Cognito redirect URI

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

The callback path is fixed at `/api/auth/callback/oidc` because Auth.js's OIDC provider id is `oidc`.

### 7. Create AUTH_SECRET and the OIDC client secret in Secrets Manager

```bash
# Auth.js session signing/encryption secret (32+ bytes)
aws secretsmanager create-secret \
  --name senko-web/auth-secret \
  --secret-string "$(openssl rand -base64 32)"

# Cognito app client secret (read it from the console or CLI)
COGNITO_CLIENT_SECRET=$(aws cognito-idp describe-user-pool-client \
  --user-pool-id "${USER_POOL_ID}" \
  --client-id "${CLIENT_ID}" \
  --query 'UserPoolClient.ClientSecret' \
  --output text)

aws secretsmanager create-secret \
  --name senko-web/oidc-client-secret \
  --secret-string "${COGNITO_CLIENT_SECRET}"
```

Note the resulting ARNs (`aws secretsmanager describe-secret --secret-id ... --query ARN --output text`).

### 8. Deploy

Set `SENKO_VERSION`, fetch the tarball, then deploy via CDK.

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

The deploy output includes `WebApiEndpoint`, the HTTP API GW URL. If you do not have a custom domain yet, either re-deploy with that URL as `webPublicUrl`, or attach an API GW Custom Domain in a separate stack and point DNS at it.

### 9. Smoke-test

```bash
WEB_URL="https://app.senko.example.com"

# 1. Anonymous session lookup (200 with body == null)
curl -i "${WEB_URL}/api/auth/session"

# 2. In a browser, open the URL below → redirected to Cognito Hosted UI →
#    after returning, /api/auth/session starts returning user info
#    ${WEB_URL}/api/auth/signin/oidc

# 3. Replay the browser cookie against the BFF-proxied backend
#    (export cookie.txt from Chrome devtools etc.)
curl -b cookie.txt -i "${WEB_URL}/api/senko/api/v1/projects"
# → 200 with a valid cookie, 401 when unauthenticated / expired
```

## env var ↔ CDK mapping

The canonical env list (required / example / description) lives in [the README env table](./README.md#environment-variables-read-by-web-lambda). The table below only shows **which CDK value backs each env var**.

| env var | Source in CDK | Notes |
| --- | --- | --- |
| `SENKO_API_BASE_URL` | `props.senkoApiBaseUrl` (literal) | No trailing `/` |
| `AUTH_URL` | `` `${props.webPublicUrl}/api/auth` `` | HTTPS required |
| `AUTH_OIDC_ISSUER` | `props.oidcIssuer` (literal) | HTTPS required. Example: `https://cognito-idp.<region>.amazonaws.com/<user-pool-id>` |
| `AUTH_OIDC_CLIENT_ID` | `props.oidcClientId` (literal) | Not a secret |
| `AUTH_OIDC_CLIENT_SECRET` | `oidcSecret.secretValue.unsafeUnwrap()` | Secrets Manager dynamic ref |
| `AUTH_SECRET` | `authSecret.secretValue.unsafeUnwrap()` | Secrets Manager dynamic ref |

Despite the name, `unsafeUnwrap()` does **not** materialize the literal in the template — it unwraps the CDK token and embeds the CFN dynamic reference string `{{resolve:secretsmanager:arn}}`. CloudFormation resolves it at deploy time and passes the real value into the Lambda environment, so the synthesized template never carries plaintext.

## Common failures and fixes

- **`redirect_uri_mismatch`** — The callback URL registered with Cognito does not match what Auth.js requests, exact-match (scheme + host + port + path). Auth.js's OIDC provider id is hard-coded to `oidc`, so the callback path is **always** `/api/auth/callback/oidc`.
- **Lambda fails at startup with `AUTH_SECRET is required`** — The Secrets Manager value is empty, the Lambda role lacks `secretsmanager:GetSecretValue`, or the ARN string has a typo. Run `aws lambda get-function-configuration --function-name <fn>` and verify env resolution.
- **`AUTH_URL must be an HTTPS URL` / `AUTH_OIDC_ISSUER must be an HTTPS URL`** — Startup fail-fast added in Task #406. The HTTP API GW `execute-api` domain is HTTPS by default, so the GW URL works as-is.
- **`Set-Cookie` too large / session never sticks** — Cognito ID/Access tokens with too many claims cause Auth.js to chunk cookies past API GW limits. Trim custom scopes / claims on the Cognito side.
- **SnapStart appears to do nothing (still cold start)** — You are likely calling `$LATEST` instead of the alias. SnapStart snapshots are scoped to published versions, so always invoke through the alias.
- **Behavior breaks only on restored snapshots** — Top-level (INIT) code that generates randomness, holds DB connections, or snapshots time will misbehave after restore. Auth.js + srvx do not perform external network calls during INIT, but if you add heavy initialization later, consult the [AWS SnapStart compatibility guide](https://docs.aws.amazon.com/lambda/latest/dg/snapstart-uniqueness.html).
- **CSP nonce breaks / browser receives stale 304s** — senko-web SSR injects a per-request nonce into script tags, so HTML responses must never be cached. If you add CloudFront in front, set an origin response policy that forces `text/html` to `Cache-Control: private, no-store`.

## Switching to a different OIDC IdP

Swap the three env vars `AUTH_OIDC_ISSUER` / `AUTH_OIDC_CLIENT_ID` / `AUTH_OIDC_CLIENT_SECRET` to retarget Keycloak / Auth0 / Google Workspace, etc. Because the HTTP API GW does not use a Cognito Authorizer, the CDK stack itself needs no changes.

- Register the same callback path (`/api/auth/callback/oidc`) on the new IdP
- Make sure the IdP allows the `openid profile email` scopes and Authorization Code Flow + PKCE
- The discovery endpoint (`<issuer>/.well-known/openid-configuration`) must publish `authorization_endpoint`, `token_endpoint`, and `jwks_uri`

The Cognito-specific steps (Hosted UI domain / `aws cognito-idp update-user-pool-client`) become unnecessary in that case.

## Optional: WAF / CloudFront in front

For production you often want WAF / CloudFront in front for rate limiting or IP allowlists. Skeleton only:

```ts
// Attach a WAF Web ACL to the HTTP API GW v2
import { CfnWebACL, CfnWebACLAssociation } from 'aws-cdk-lib/aws-wafv2'

const acl = new CfnWebACL(this, 'WebAcl', {
  scope: 'REGIONAL',
  defaultAction: { allow: {} },
  visibilityConfig: {
    cloudWatchMetricsEnabled: true,
    metricName: 'senko-web-acl',
    sampledRequestsEnabled: true,
  },
  rules: [/* AWSManagedRulesCommonRuleSet etc. */],
})

new CfnWebACLAssociation(this, 'WebAclAssoc', {
  resourceArn: `arn:aws:apigateway:${this.region}::/apis/${httpApi.apiId}/stages/$default`,
  webAclArn: acl.attrArn,
})
```

If you front this with CloudFront, the only must-do is **never cache `text/html`** (the per-request CSP nonce will break otherwise). Static assets under `/_build/*` are safe to cache aggressively.

## Related docs

- [senko-web Deployment Guide (README)](./README.md) — env vars / tarball download / overall map
- [senko backend Deploy](../server-remote/deploy.md), [AWS deployment example](../server-remote/aws-deployment.md)
- [OIDC Authentication Guide](../server-remote/auth-oidc.md)
