# REST API リファレンス

`senko serve` が提供する HTTP API のエンドポイント一覧。CLI とリモートクライアントはすべてこの API 経由でサーバとやり取りします。

## 認証

すべての `/api/v1/*` エンドポイントは認証必須 (`/api/v1/health` を除く)。認証モードはサーバ側の設定で **3 つのうちのどれか 1 つ** (pairwise 排他):

| モード | クライアント側の送り方 | サーバ設定 |
|---|---|---|
| API キー | `Authorization: Bearer <key>` (API キー or master_key) | `[server.auth.api_key]` |
| OIDC JWT | `Authorization: Bearer <jwt>` | `[server.auth.oidc]` |
| 信頼ヘッダ | `x-senko-user-sub: ...` 等 (API Gateway が注入) | `[server.auth.trusted_headers]` |

**master 権限** (`is_master`) は一部エンドポイント (ユーザ CRUD 等) で必須。どう与えるかはモードごとに異なる:

- API キーモード: `[server.auth.api_key] master_key` の値を Bearer で送ったリクエスト
- OIDC モード: `[server.auth.oidc] master_group` に指定したグループを JWT の `groups_claim` に含む
- 信頼ヘッダモード: `[server.auth.trusted_headers] master_group` に指定した値が `groups_header` に含まれる

OIDC / 信頼ヘッダモードでは **初回認証時にユーザが JIT 自動登録** されるため、事前のユーザ作成は不要。API キーモードでのみ master_key を使った明示的な user 発行が必要 (または行なうと便利)。

## エラーレスポンス形式

```json
{
  "error": {
    "code": "not_found",
    "message": "Task 42 not found"
  }
}
```

HTTP ステータス:

| コード | 意味 |
|---|---|
| 400 | バリデーションエラー (不正な入力、状態遷移違反) |
| 401 | 認証失敗 |
| 403 | 認可失敗 (project member ではない、role 不足) |
| 404 | リソース未発見 |
| 409 | 競合 (unique 制約、循環依存) |
| 500 | 内部エラー |

## バージョンヘッダ

すべてのレスポンスに以下が付与される:

```
X-Senko-Version: 1.0.0
```

クライアントはこれを確認してサーバ互換性を判定できます。

## エンドポイント一覧

### ヘルスチェック・設定

| Method | Path | 認証 | 説明 |
|---|---|---|---|
| GET | `/api/v1/health` | 不要 | `{"status":"ok"}` |
| GET | `/api/v1/config` | 要 | マージ済み config の JSON |
| GET | `/auth/config` | 不要 | CLI login が使う OIDC issuer/client_id |
| GET | `/auth/me` | 要 | 現在のユーザ情報 |
| POST | `/auth/token` | (特殊) | OAuth PKCE フロー後に CLI が叩くトークン交換 |
| GET | `/auth/sessions` | 要 | 自分のセッション一覧 |
| DELETE | `/auth/sessions` | 要 | 全セッション revoke |
| DELETE | `/auth/sessions/{id}` | 要 | 特定セッション revoke |

### ユーザ管理

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/users` | 一覧 (**master 権限必須**) |
| POST | `/api/v1/users` | 明示的な user 発行 (**master 権限必須**)。OIDC / 信頼ヘッダモードでは JIT 自動登録があるため多くの場合不要 |
| GET | `/api/v1/users/{id}` | 取得 (**master 権限必須**) |
| PUT | `/api/v1/users/{id}` | 更新 (**master 権限必須**) |
| DELETE | `/api/v1/users/{id}` | 削除 (**master 権限必須**) |

### API キー管理

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/users/{user_id}/api-keys` | 発行済み API キー一覧 |
| POST | `/api/v1/users/{user_id}/api-keys` | API キー発行 (`name` / `device_name`) |
| DELETE | `/api/v1/users/{user_id}/api-keys/{id}` | revoke |

### プロジェクト

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/projects` | 自分が member の project 一覧 |
| POST | `/api/v1/projects` | 作成 |
| GET | `/api/v1/projects/{id}` | 取得 |
| DELETE | `/api/v1/projects/{id}` | 削除 (owner 必須) |
| GET | `/api/v1/projects/{id}/stats` | `{draft,todo,in_progress,completed}` カウント |

### プロジェクトメンバー

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/members` | 一覧 |
| POST | `/api/v1/projects/{project_id}/members` | 追加 |
| GET | `/api/v1/projects/{project_id}/members/{user_id}` | 取得 |
| PUT | `/api/v1/projects/{project_id}/members/{user_id}` | role 更新 |
| DELETE | `/api/v1/projects/{project_id}/members/{user_id}` | 削除 |

### タスク

`{project_id}` は project の **ID** (数値)。

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/tasks` | 一覧 (クエリ `status`, `tag`, `ready`, `contract`, `id_min`, `id_max`, `limit`, `offset`, `metadata` …) |
| POST | `/api/v1/projects/{project_id}/tasks` | 作成 |
| GET | `/api/v1/projects/{project_id}/tasks/{id}` | 取得 |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}` | 部分更新 |
| DELETE | `/api/v1/projects/{project_id}/tasks/{id}` | 削除 |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}/save` | 冪等な idempotent save |
| GET | `/api/v1/projects/{project_id}/tasks/{id}/transition/preview` | 次に遷移できる状態 |
| POST | `/api/v1/projects/{project_id}/tasks/next` | `senko task next` 相当 |
| GET | `/api/v1/projects/{project_id}/tasks/next/preview` | 選ばれる予定のタスクを覗き見 |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/ready` | draft → todo |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/start` | todo → in_progress |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/complete` | in_progress → completed |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/cancel` | → canceled |
| GET | `/api/v1/projects/{project_id}/tasks/{id}/deps` | 依存一覧 |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/deps` | 依存追加 |
| PUT | `/api/v1/projects/{project_id}/tasks/{id}/deps` | 依存全置換 |
| DELETE | `/api/v1/projects/{project_id}/tasks/{id}/deps/{dep_id}` | 依存削除 |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/dod/check` | DoD check (`{"index": N}`) |
| POST | `/api/v1/projects/{project_id}/tasks/{id}/dod/uncheck` | DoD uncheck |

### Contract

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/contracts` | 一覧 |
| POST | `/api/v1/projects/{project_id}/contracts` | 作成 |
| GET | `/api/v1/projects/{project_id}/contracts/{id}` | 取得 |
| PUT | `/api/v1/projects/{project_id}/contracts/{id}` | 更新 |
| DELETE | `/api/v1/projects/{project_id}/contracts/{id}` | 削除 |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/dod/check` | DoD check |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/dod/uncheck` | DoD uncheck |
| GET | `/api/v1/projects/{project_id}/contracts/{id}/notes` | Notes 一覧 |
| POST | `/api/v1/projects/{project_id}/contracts/{id}/notes` | Note 追加 |

### Metadata fields

| Method | Path | 備考 |
|---|---|---|
| GET | `/api/v1/projects/{project_id}/metadata-fields` | 一覧 |
| POST | `/api/v1/projects/{project_id}/metadata-fields` | 追加 |
| DELETE | `/api/v1/projects/{project_id}/metadata-fields/{name}` | 削除 |

## リクエスト/レスポンスの形

Task / Contract のフィールド形式は `senko task get` / `senko contract get` の JSON 出力と同一です。詳細は [reference/data-model.md](data-model.md) と [reference/cli.md](cli.md) を参照。

例: タスク作成リクエスト

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

レスポンス:

```json
{
  "id": 42,
  "project_id": 1,
  "task_number": 7,
  "title": "Implement webhook",
  ...
}
```

## Hook のリクエスト単位発火

認証済みのリクエストが状態遷移を起こすと、サーバ上の `[server.remote.<action>.hooks.*]` (または relay の場合 `[server.relay.<action>.hooks.*]`) が発火します。hook の envelope 形式は [reference/hooks.md](hooks.md) 参照。

## レート制限

現時点ではサーバ側の組み込みレートリミットはなし。必要なら API Gateway / nginx 側で制御してください。
