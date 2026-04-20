# REST API Reference

Every HTTP endpoint exposed by `senko serve`. The CLI and all remote clients talk to the server through this API.

## Authentication

Every `/api/v1/*` endpoint requires authentication (except `/api/v1/health`). The auth mode is server-configured and is **one of three** (pairwise exclusive):

| Mode | How the client sends it | Server config |
|---|---|---|
| API key | `Authorization: Bearer <key>` (API key or master_key) | `[server.auth.api_key]` |
| OIDC JWT | `Authorization: Bearer <jwt>` | `[server.auth.oidc]` |
| Trusted headers | `x-senko-user-sub: ...` etc. (injected by an API Gateway) | `[server.auth.trusted_headers]` |

**Master privilege** (`is_master`) is required for certain endpoints (user CRUD, etc.). How it's granted differs by mode:

- API key mode: send the value of `[server.auth.api_key] master_key` as Bearer.
- OIDC mode: the JWT's `groups_claim` must contain the group named by `[server.auth.oidc] master_group`.
- Trusted headers mode: `groups_header` must include `[server.auth.trusted_headers] master_group`.

In OIDC / trusted-headers modes, **users are JIT-provisioned on first auth**, so no pre-issuance is required. Explicit user creation via `master_key` is only needed (or helpful) in API-key mode.

## Error Response Format

```json
{
  "error": {
    "code": "not_found",
    "message": "Task 42 not found"
  }
}
```

HTTP status codes:

| Code | Meaning |
|---|---|
| 400 | Validation error (bad input, illegal state transition) |
| 401 | Authentication failed |
| 403 | Authorization failed (not a project member, insufficient role) |
| 404 | Resource not found |
| 409 | Conflict (unique constraint, dependency cycle) |
| 500 | Internal error |

## Version Header

Every response carries:

```
X-Senko-Version: 1.0.0
```

Clients can use it to check server compatibility.

## Endpoint List

### Health and Config

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/api/v1/health` | none | `{"status":"ok"}` |
| GET | `/api/v1/config` | required | Merged config as JSON |
| GET | `/auth/config` | none | OIDC issuer/client_id used by CLI login |
| GET | `/auth/me` | required | Current user info |
| POST | `/auth/token` | (special) | Token exchange called by the CLI after the OAuth PKCE flow |
| GET | `/auth/sessions` | required | List my sessions |
| DELETE | `/auth/sessions` | required | Revoke every session |
| DELETE | `/auth/sessions/{id}` | required | Revoke a specific session |

### User Management

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/users` | List (**master required**) |
| POST | `/api/v1/users` | Explicit user creation (**master required**). Usually unnecessary under OIDC / trusted headers thanks to JIT provisioning |
| GET | `/api/v1/users/{id}` | Get (**master required**) |
| PUT | `/api/v1/users/{id}` | Update (**master required**) |
| DELETE | `/api/v1/users/{id}` | Delete (**master required**) |

### API Key Management

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/users/{user_id}/api-keys` | List issued API keys |
| POST | `/api/v1/users/{user_id}/api-keys` | Issue an API key (`name` / `device_name`) |
| DELETE | `/api/v1/users/{user_id}/api-keys/{key_id}` | Revoke. The `user_id` in the path must equal the caller (IDOR scoping) |

### Projects

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/projects` | Projects I'm a member of |
| POST | `/api/v1/projects` | Create |
| GET | `/api/v1/projects/{id}` | Get |
| DELETE | `/api/v1/projects/{id}` | Delete (owner required) |
| GET | `/api/v1/projects/{id}/stats` | `{draft,todo,in_progress,completed}` counts |

### Project Members

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/members` | List |
| POST | `/api/v1/projects/{project_id}/members` | Add |
| GET | `/api/v1/projects/{project_id}/members/{user_id}` | Get |
| PUT | `/api/v1/projects/{project_id}/members/{user_id}` | Update role |
| DELETE | `/api/v1/projects/{project_id}/members/{user_id}` | Remove |

### Tasks

`{project_id}` is the project's **ID** (numeric).

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/tasks` | List (query: `status`, `tag`, `ready`, `contract`, `id_min`, `id_max`, `limit`, `offset`, `metadata`, …) |
| POST | `/api/v1/projects/{project_id}/tasks` | Create |
| GET | `/api/v1/projects/{project_id}/tasks/{id}` | Get |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}` | Partial update |
| DELETE | `/api/v1/projects/{project_id}/tasks/{id}` | Delete |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}/_save` | Idempotent save |
| GET | `/api/v1/projects/{project_id}/tasks/{id}/preview-transition` | Which transitions are currently allowed |
| POST | `/api/v1/projects/{project_id}/tasks/next` | Equivalent of `senko task next` |
| GET | `/api/v1/projects/{project_id}/tasks/preview-next` | Peek at the task that would be picked |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/ready` | draft → todo |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/start` | todo → in_progress |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/complete` | in_progress → completed |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/cancel` | → canceled |
| GET | `/api/v1/projects/{project_id}/tasks/{id}/deps` | List dependencies |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/deps` | Add a dependency |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}/deps` | Replace dependencies |
| DELETE | `/api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}` | Remove a dependency |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/check` | DoD check (1-based index) |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/dod/{index}/uncheck` | DoD uncheck |

### Contracts

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/contracts` | List |
| POST | `/api/v1/projects/{project_id}/contracts` | Create |
| GET | `/api/v1/projects/{project_id}/contracts/{id}` | Get |
| PUT | `/api/v1/projects/{project_id}/contracts/{id}` | Update |
| DELETE | `/api/v1/projects/{project_id}/contracts/{id}` | Delete |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/dod/{index}/check` | DoD check (1-based index) |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/dod/{index}/uncheck` | DoD uncheck |
| GET | `/api/v1/projects/{project_id}/contracts/{id}/notes` | List notes |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/notes` | Add a note |

### Metadata Fields

| Method | Path | Notes |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/metadata-fields` | List |
| POST | `/api/v1/projects/{project_id}/metadata-fields` | Add |
| DELETE | `/api/v1/projects/{project_id}/metadata-fields/{name}` | Delete |

## Request / Response Shape

Task / Contract fields are identical to the JSON output of `senko task get` / `senko contract get`. See [Data Model](data-model.md) and [CLI Reference](cli.md) for details.

Example — task creation request:

```http
POST /api/v1/projects/1/tasks
Authorization: Bearer sk_...
Content-Type: application/json

{
  "title": "Implement webhook",
  "background": "External integration",
  "priority": "P1",
  "definition_of_done": ["Tests pass", "Docs updated"],
  "in_scope": ["endpoint"],
  "out_of_scope": ["GraphQL"],
  "tags": ["backend"],
  "metadata": {"estimate_points": 5}
}
```

Response:

```json
{
  "id": 42,
  "project_id": 1,
  "task_number": 7,
  "title": "Implement webhook",
  ...
}
```

## Per-Request Hook Firing

When an authenticated request triggers a state transition, the server fires `[server.remote.<action>.hooks.*]` (or `[server.relay.<action>.hooks.*]` in relay mode). See [Hooks Reference](hooks.md) for envelope shape.

## Rate Limiting

There's no built-in rate limiting at present. If you need it, apply it at the API Gateway / nginx layer.
