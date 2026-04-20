# AWS Deployment (API Gateway + Cognito + Lambda Web Adapter)

A setup where API Gateway HTTP API terminates JWT verification and passes identity to senko (running on Lambda) via trusted headers.

## Architecture

```
Client ──[Authorization: Bearer <Cognito JWT>]──┐
                                                 ▼
                                        API Gateway HTTP API
                                            │
                                            ├─ Cognito JWT Authorizer (verifies the JWT)
                                            ├─ Parameter Mapping (JWT claims → x-senko-* headers)
                                            ▼
                                        Lambda (Web Adapter)
                                            │
                                            │  x-senko-user-sub: <sub>
                                            │  x-senko-user-name: <name>
                                            │  x-senko-user-email: <email>
                                            │  x-senko-user-groups: <groups>
                                            ▼
                                        senko serve (trusted_headers mode)
                                            │
                                            └─ Backend: Aurora / RDS PostgreSQL
```

- **API Gateway**: TLS termination + JWT verification + header transformation.
- **Lambda Web Adapter**: lets a normal HTTP server (`senko serve`) run inside Lambda.
- **senko**: runs in `trusted_headers` mode, derives identity solely from headers.
- **DB**: RDS / Aurora PostgreSQL (recommended in a separate VPC).

## Prerequisites

- An AWS account
- A Cognito User Pool
- API Gateway HTTP API (use HTTP API, not REST API)
- The [Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter) layer
- The `senko` binary (`postgres` + `aws-secrets` features)

## Step 1. Cognito User Pool

Create the user pool and note:

- **User Pool ID**: `ap-northeast-1_XXXXXXXXX`
- **Issuer URL**: `https://cognito-idp.{region}.amazonaws.com/{user-pool-id}`

Create an app client:

- Public client (no client secret)
- Enable PKCE
- Callback URL: `http://127.0.0.1:<port>/callback` (the local callback port for CLI login)

## Step 2. The senko Lambda

### Packaging

- Build the Rust binary for `aarch64-unknown-linux-musl` or `x86_64-unknown-linux-musl` (with `postgres` + `aws-secrets`).
- Add the Lambda Web Adapter shared layer.
- The handler is a `bootstrap`-equivalent wrapper script that runs `senko serve --host 127.0.0.1 --port 8080`.
- Pick a timeout in the 30-second to 15-minute range (long-lived connections aren't expected).

### Environment variables

```
SENKO_POSTGRES_URL = (set via rds_secrets_arn instead)
PORT               = 8080    # Lambda Web Adapter reads this
```

### IAM

- `secretsmanager:GetSecretValue` for the RDS secret
- VPC configuration if the Lambda needs to reach RDS inside a VPC
- CloudWatch Logs write permission

### Config file

Inside the Lambda, `.senko/config.toml`:

```toml
[backend.postgres]
# Recommended: point at a secret for the RDS Proxy endpoint. Proxy aggregates the
# connection pool, so each Lambda keeps a short-lived connection. Without Proxy,
# cap max_connections at 1–3 and throttle concurrent executions externally.
rds_secrets_arn = "arn:aws:secretsmanager:...:secret:rds-proxy/senko"
sslrootcert     = "/opt/rds-ca-bundle.pem"
max_connections = 5

[server.auth.trusted_headers]
subject_header      = "x-senko-user-sub"
name_header         = "x-senko-user-name"
email_header        = "x-senko-user-email"
groups_header       = "x-senko-user-groups"
# For super-admin: match a Cognito group name
master_group        = "senko-admins"
oidc_issuer_url     = "https://cognito-idp.ap-northeast-1.amazonaws.com/ap-northeast-1_XXXXXXXXX"
oidc_client_id      = "your-app-client-id"

# Only one of (api_key / oidc / trusted_headers) can be enabled.
# Since this setup uses trusted_headers, don't configure [server.auth.api_key] or [server.auth.oidc].

[log]
format = "json"
level  = "info"
```

## Step 3. API Gateway HTTP API

### Authorizer

- Type: **JWT Authorizer**
- Issuer URL: the Cognito issuer
- Audience: the app client ID
- Identity source: `$request.header.Authorization`

### Routes

A single `$default` route routes everything to the Lambda:

```
ANY / {proxy+}   →   Lambda integration (HTTP API)
```

Attach the Authorizer to the route.

### Parameter Mapping (Authorizer → headers)

Add the following under `Overwrite request headers`:

| Header | Value |
|---|---|
| `x-senko-user-sub` | `$context.authorizer.claims.sub` |
| `x-senko-user-name` | `$context.authorizer.claims.cognito:username` |
| `x-senko-user-email` | `$context.authorizer.claims.email` |
| `x-senko-user-groups` | `$context.authorizer.claims.cognito:groups` |

**Important**: set mode to "Overwrite" so that client-supplied `x-senko-*` headers get overwritten. "Append" is unsafe.

## Step 4. VPC / RDS

- Put the senko Lambda in a private subnet.
- Put RDS (Aurora PostgreSQL) in the same VPC, reachable only from the senko Lambda.
- You need a VPC endpoint for `secretsmanager:*` (NAT Gateway works too, but an endpoint is preferred).

## Step 5. CLI Side

Team member's CLI config:

```toml
[cli.remote]
url = "https://senko.example.com"   # API Gateway custom domain
```

Log in:

```bash
senko auth login
```

Behavior: senko fetches `oidc_issuer_url` / `oidc_client_id` from the server via `GET /auth/config` (= API Gateway → Lambda → senko), logs in against Cognito, and sends the resulting JWT as Bearer. The API Gateway verifies it, rewrites headers, and passes the request to senko.

## Operations Tips

- **Cold start**: 1–3 seconds for the first Lambda invocation plus DB connect cost. Consider ProvisionedConcurrency.
- **Connection pool**: Lambda spins up separate instances per concurrent execution; if every Lambda pools `max_connections` directly against RDS, **RDS can exhaust connections immediately**. The **recommended setup is to put RDS Proxy in between** (Proxy shares the pool and Lambda holds short-lived connections). Without Proxy, cap `[backend.postgres] max_connections` at 1–3 and limit concurrency via ProvisionedConcurrency / reserved concurrency.
- **Migrations**: when a new Lambda version deploys, unapplied migrations run automatically. Use canary deployment to minimize cross-version overlap. RDS Proxy softens connection pressure when multiple versions coexist.
- **Super-admin**: create a Cognito group like `senko-admins` and match `master_group`. Put initial admins in that group; they get full user-list and project-management capabilities the moment they log in.
- **Audit logs**: `[server.remote.*.hooks.audit]` can stream from Lambda to CloudWatch Metrics or EventBridge.

## Troubleshooting

| Symptom | What to check |
|---|---|
| 401 Unauthorized | Authorizer config on the API Gateway (JWT audience / issuer) |
| senko reports `user unknown` | Parameter Mapping not firing; check API Gateway logs for `x-senko-*` |
| Can't connect to RDS | Lambda SG / subnet, RDS (or RDS Proxy) SG |
| `secretsmanager:GetSecretValue` denied | Lambda IAM role + VPC endpoint policy |
| RDS `max_connections` exhausted | **First: add RDS Proxy.** Without it, shrink `[backend.postgres] max_connections` and limit concurrency via ProvisionedConcurrency / reserved concurrency |
| Slow cold start | Consider ProvisionedConcurrency |

## Next Steps

- Trusted-headers details → [Trusted Headers Authentication](auth-trusted-headers.md)
- CloudWatch audit via hooks → [`[server.remote.*]` Hook Examples](hooks.md)
