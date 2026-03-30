# CLIリファレンス

[English](CLI.md) | [READMEに戻る](README.ja.md)

## グローバルオプション

```
--output <FORMAT>       json または text（デフォルト: json）
--project-root <PATH>   プロジェクトルート（省略時は自動検出）
--config <PATH>         設定ファイルのパス（環境変数: SENKO_CONFIG、デフォルト: .senko/config.toml）
--dry-run               実行せずに結果を表示（状態変更コマンドのみ）
--log-dir <PATH>        ログ出力ディレクトリを上書き（デフォルト: $XDG_STATE_HOME/senko）
```

> **注意**: `--output` と `--dry-run` はグローバルフラグです。サブコマンドの**前**に配置してください: `senko --output text list`

## `add` – タスク作成

```bash
senko add --title "ドキュメント作成" --priority p0
senko add --title "バグ修正" \
  --background "ユーザーから500エラーの報告" \
  --definition-of-done "ログに500エラーなし" \
  --in-scope "エラーハンドラ" \
  --out-of-scope "リファクタリング" \
  --tag backend --tag urgent
```

新規タスクは `draft` ステータスで作成されます。デフォルト優先度は `p2`。

## `list` – タスク一覧

```bash
senko list                    # 全タスク
senko list --status todo      # ステータスで絞り込み
senko list --ready            # 依存解決済みのtodoタスク
senko list --tag backend      # タグで絞り込み
```

CLIフラグのステータス値はスネークケース: `todo`, `in_progress`, `completed`, `canceled`, `draft`

## `get <id>` – タスク詳細

```bash
senko get 1
```

> `get` はJSON出力のみ（`--output text` 非対応）。

## `next` – 次のタスクを開始

依存タスクがすべて完了済みの最高優先度 `todo` タスクを選択し、`in_progress` に変更します。

```bash
senko next
senko next --session-id "session-abc"
```

選択順序: 優先度（P0優先）→ 作成日時 → ID

## `edit <id>` – タスク編集

```bash
# スカラーフィールド
senko edit 1 --title "新しいタイトル"
senko edit 1 --status todo
senko edit 1 --priority p0

# 配列フィールド（タグ、完了定義、スコープ）
senko edit 1 --add-tag "urgent"
senko edit 1 --remove-tag "old"
senko edit 1 --set-tags "a" "b"         # 全置換

# 完了定義（Definition of Done）
senko edit 1 --add-definition-of-done "ユニットテストを書く"

# PR URL
senko edit 1 --pr-url "https://github.com/org/repo/pull/42"
senko edit 1 --clear-pr-url
```

## `complete <id>` – タスク完了

```bash
senko complete 1
senko complete 1 --skip-pr-check    # PR検証をスキップ
```

未チェックのDoD項目がある場合は失敗します。先に `dod check` でマークしてください。

`completion_mode = "pr_then_complete"` 設定時は、PRがマージ済みであることも検証します（`auto_merge = false` の場合は承認も確認）。`--skip-pr-check` で検証をスキップできます。

## `cancel <id>` – タスクキャンセル

```bash
senko cancel 1 --reason "スコープ外"
```

## `dod` – 完了定義（DoD）の管理

```bash
senko dod check <task_id> <index>      # DoD項目をチェック（1始まり）
senko dod uncheck <task_id> <index>    # DoD項目のチェックを外す
```

## `deps` – 依存関係管理

```bash
senko deps add 5 --on 3        # タスク5がタスク3に依存
senko deps remove 5 --on 3     # 依存を削除
senko deps set 5 --on 1 2 3    # 依存を一括設定
senko deps list 5              # タスク5の依存一覧
```

## `config` – 設定の表示・初期化

```bash
senko config              # 現在の設定を表示（JSON）
senko --output text config # 現在の設定を表示（テキスト）
senko config --init       # テンプレート .senko/config.toml を生成
```

現在の設定値（未設定項目はデフォルト値）を表示します。`--init` でコメント付きテンプレートファイルを生成します。

## `skill-install` – Claude Code連携

```bash
senko skill-install
```

Claude Code連携用のスキル定義を `.claude/skills/senko/` に生成します。

## `serve` – JSON APIサーバーを起動

```bash
senko serve                # 127.0.0.1:3142 でリッスン
senko serve --port 8080    # 127.0.0.1:8080 でリッスン
senko serve --host 0.0.0.0 # 0.0.0.0:3142 でリッスン（全インターフェース）
```

| オプション | 説明 |
|--------|-------------|
| `--port <PORT>` | リッスンポート（環境変数: `SENKO_PORT`、デフォルト: `3142`） |
| `--host <ADDR>` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`）（環境変数: `SENKO_HOST`、デフォルト: `127.0.0.1`） |

`/api/v1/...` 配下で全タスク操作（CRUD、ステータス遷移、依存関係、DoD、設定、統計）をJSON REST APIとして提供します。CLIと同様にhooksが発火します。

## `web` – 読み取り専用Webビューアを起動

```bash
senko web                # 127.0.0.1:3141 でリッスン
senko web --port 8080    # 127.0.0.1:8080 でリッスン
senko web --host 0.0.0.0 # 0.0.0.0:3141 でリッスン（全インターフェース）
```

| オプション | 説明 |
|--------|-------------|
| `--port <PORT>` | リッスンポート（環境変数: `SENKO_PORT`、デフォルト: `3141`） |
| `--host <ADDR>` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`）（環境変数: `SENKO_HOST`、デフォルト: `127.0.0.1`） |

## Docker

### Dockerfile

```dockerfile
FROM debian:bookworm-slim
ARG SENKO_VERSION=0.10.0
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && case "${TARGETARCH}" in \
       amd64) TARGET="x86_64-unknown-linux-musl" ;; \
       arm64) TARGET="aarch64-unknown-linux-musl" ;; \
       *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1 ;; \
     esac \
  && curl -fsSL "https://github.com/hisamekms/senko/releases/download/v${SENKO_VERSION}/senko-v${SENKO_VERSION}-${TARGET}.tar.gz" \
     | tar xz -C /usr/local/bin senko
WORKDIR /project
ENTRYPOINT ["senko"]
```

> **注意**: `TARGETARCH` はDocker BuildKitがビルドプラットフォームに基づいて自動設定します。このDockerfileは `amd64` と `arm64` の両方に対応しています。

### ビルドと実行

```bash
# イメージをビルド
docker build -t senko .

# コマンドを実行
docker run --rm -v "$(pwd)/.senko:/project/.senko" senko list

# APIサーバーを起動
docker run --rm -p 3142:3142 \
  -v "$(pwd)/.senko:/project/.senko" \
  senko serve --host 0.0.0.0
```

### ボリュームマウントによるデータ永続化

senkoはSQLiteデータベースと設定を `.senko/` ディレクトリに保存します。コンテナ間でデータを永続化するには、このディレクトリをボリュームとしてマウントしてください:

```
-v "$(pwd)/.senko:/project/.senko"
```

マウント対象:
- `tasks.db` – SQLiteデータベース
- `config.toml` – フックとワークフローの設定

ボリュームマウントなしでは、コンテナ停止時にすべてのデータが失われます。

## フック – タスク状態変更時の自動アクション

フックはCLIコマンドがタスク状態を変更した際に自動実行されるシェルコマンドです。デーモン不要で、fire-and-forget（発火後即座に制御を返す）方式で子プロセスとして実行されるため、CLIをブロックしません。

### 設定

`.senko/config.toml` にフックを定義します:

```toml
[hooks]
on_task_added = "echo '新しいタスク' | notify-send -"
on_task_ready = "curl -X POST https://example.com/ready"
on_task_started = "slack-notify started"
on_task_completed = "curl -X POST https://example.com/webhook"
on_task_canceled = "echo canceled"
```

イベントごとに複数コマンドを配列で指定できます:

```toml
[hooks]
on_task_completed = ["notify-send '完了'", "curl https://example.com/done"]
```

| フック | トリガー |
|------|---------|
| `on_task_added` | `senko add` で新しいタスクを作成 |
| `on_task_ready` | `senko ready` でタスクを draft から todo に遷移 |
| `on_task_started` | `senko start` または `senko next` でタスクを開始 |
| `on_task_completed` | `senko complete` でタスクを完了 |
| `on_task_canceled` | `senko cancel` でタスクをキャンセル |

フックは **stdin** でイベントペイロード（JSON）を受け取り、`sh -c` で実行されます。

### イベントペイロード

フックのstdinに渡されるJSONオブジェクト（「フックエンベロープ」）:

```json
{
  "runtime": "cli",
  "backend": {
    "type": "sqlite",
    "db_file_path": "/path/to/project/.senko/senko.db"
  },
  "project": {
    "id": 1,
    "name": "default"
  },
  "user": {
    "id": 1,
    "name": "default"
  },
  "event": {
    "event_id": "550e8400-e29b-41d4-a716-446655440000",
    "event": "task_completed",
    "timestamp": "2026-03-24T12:00:00Z",
    "from_status": "in_progress",
    "task": {
      "id": 7,
      "project_id": 1,
      "title": "Webhookハンドラの実装",
      "background": null,
      "description": "外部連携用のWebhookエンドポイントを追加",
      "plan": null,
      "priority": "P1",
      "status": "completed",
      "assignee_session_id": null,
      "assignee_user_id": null,
      "created_at": "2026-03-24T10:00:00Z",
      "updated_at": "2026-03-24T12:00:00Z",
      "started_at": "2026-03-24T10:30:00Z",
      "completed_at": "2026-03-24T12:00:00Z",
      "canceled_at": null,
      "cancel_reason": null,
      "branch": "feature/webhook",
      "pr_url": "https://github.com/org/repo/pull/42",
      "metadata": null,
      "definition_of_done": [
        { "content": "ユニットテストを書く", "checked": true },
        { "content": "APIドキュメントを更新", "checked": true }
      ],
      "in_scope": ["RESTエンドポイント"],
      "out_of_scope": ["GraphQLサポート"],
      "tags": ["backend", "api"],
      "dependencies": [3, 5]
    },
    "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5 },
    "ready_count": 2,
    "unblocked_tasks": [{ "id": 3, "title": "次のタスク", "priority": "P1", "metadata": null }]
  }
}
```

#### エンベロープフィールド

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `runtime` | string | `"cli"` または `"api"` |
| `backend` | object | バックエンド情報（`type` およびバックエンド固有フィールド） |
| `project` | object | プロジェクト情報: `id`（integer）と `name`（string） |
| `user` | object | ユーザー情報: `id`（integer）と `name`（string） |
| `event` | object | イベントペイロード（下記参照） |

`project` と `user` は現在のconfigを反映します。`config.toml` で `[project] name` や `[user] name` が設定されている場合、対応する名前がバックエンドから解決されます。未設定の場合はデフォルトレコード（id=1）が使用されます。

#### `event` フィールド

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `event_id` | string | UUID v4 一意識別子 |
| `event` | string | イベント名（例: `"task_added"`, `"task_completed"`） |
| `timestamp` | string | ISO 8601（RFC 3339）タイムスタンプ |
| `from_status` | string \| null | 遷移前のステータス |
| `task` | object | タスクオブジェクト全体（`senko get` と同じスキーマ — 下記参照） |
| `stats` | object | ステータス別タスク数（`{"todo": 3, "completed": 5, ...}`） |
| `ready_count` | integer | 依存解決済みの `todo` タスク数 |
| `unblocked_tasks` | array \| null | このイベントで新たにブロック解除されたタスク（`task_completed` のみ） |

#### `task` オブジェクト

イベントペイロードに含まれるタスクオブジェクト全体。`senko get` の出力と同じスキーマです。

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `id` | integer | タスクID |
| `project_id` | integer | プロジェクトID |
| `title` | string | タスクタイトル |
| `background` | string \| null | 背景情報 |
| `description` | string \| null | タスクの説明 |
| `plan` | string \| null | 実装計画 |
| `priority` | string | `"P0"` – `"P3"` |
| `status` | string | `"draft"`, `"todo"`, `"in_progress"`, `"completed"`, `"canceled"` |
| `assignee_session_id` | string \| null | 割り当てセッションID |
| `assignee_user_id` | integer \| null | 割り当てユーザーID |
| `created_at` | string | ISO 8601 タイムスタンプ |
| `updated_at` | string | ISO 8601 タイムスタンプ |
| `started_at` | string \| null | ISO 8601 タイムスタンプ（タスク開始日時） |
| `completed_at` | string \| null | ISO 8601 タイムスタンプ（タスク完了日時） |
| `canceled_at` | string \| null | ISO 8601 タイムスタンプ（タスクキャンセル日時） |
| `cancel_reason` | string \| null | キャンセル理由 |
| `branch` | string \| null | 関連gitブランチ |
| `pr_url` | string \| null | プルリクエストURL |
| `metadata` | object \| null | 任意のJSONメタデータ |
| `definition_of_done` | array | DoD項目のリスト（下記参照） |
| `in_scope` | array | スコープ内の項目（文字列） |
| `out_of_scope` | array | スコープ外の項目（文字列） |
| `tags` | array | タグ文字列 |
| `dependencies` | array | 依存タスクID（整数） |

`definition_of_done` の各要素:

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `content` | string | DoD項目の内容 |
| `checked` | boolean | チェック済みかどうか |

#### `unblocked_tasks` の要素

`task_completed` イベントで、タスク完了により他のタスクのブロックが解除された場合に含まれます。

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `id` | integer | タスクID |
| `title` | string | タスクタイトル |
| `priority` | string | `"P0"` – `"P3"` |
| `metadata` | object \| null | タスクメタデータ（任意のJSON） |

| レベル | 説明 |
|-------|-------------|
| `INFO` | 通常の操作（起動、イベント検出、フック実行成功） |
| `WARN` | フックが非ゼロ終了コードを返した |
| `ERROR` | フックの実行に失敗した |

## 環境変数

全設定は **CLIフラグ > 環境変数 > config.toml > デフォルト値** の優先順位で適用されます。

### サーバー

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `SENKO_PORT` | `web` / `serve` コマンドのポート | `3141`（web）/ `3142`（serve） |
| `SENKO_HOST` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`） | `127.0.0.1` |
| `SENKO_PROJECT_ROOT` | プロジェクトルートディレクトリ | 自動検出 |
| `SENKO_CONFIG` | 設定ファイルのパス | `.senko/config.toml` |

### ワークフロー

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `SENKO_COMPLETION_MODE` | `merge_then_complete` または `pr_then_complete` | `merge_then_complete` |
| `SENKO_AUTO_MERGE` | `true` または `false` | `true` |

### バックエンド

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `SENKO_API_URL` | APIサーバーURL（設定するとSQLiteの代わりにHTTPバックエンドを使用） | _（未設定 = SQLite）_ |
| `SENKO_HOOK_MODE` | `server`、`client`、または `both` | `server` |

### ログ

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `SENKO_LOG_DIR` | フックログの出力ディレクトリ | `$XDG_STATE_HOME/senko` |

### フック

| 変数 | 説明 |
|------|------|
| `SENKO_HOOK_ON_TASK_ADDED` | タスク作成時に実行するシェルコマンド |
| `SENKO_HOOK_ON_TASK_READY` | タスクがready時に実行するシェルコマンド |
| `SENKO_HOOK_ON_TASK_STARTED` | タスク開始時に実行するシェルコマンド |
| `SENKO_HOOK_ON_TASK_COMPLETED` | タスク完了時に実行するシェルコマンド |
| `SENKO_HOOK_ON_TASK_CANCELED` | タスクキャンセル時に実行するシェルコマンド |

フック環境変数は `config.toml` の `[hooks]` セクションの設定をオーバーライドします。

### 例: Dockerデプロイ

```bash
docker run -e SENKO_PORT=8080 \
  -e SENKO_HOST=0.0.0.0 \
  -e SENKO_HOOK_ON_TASK_COMPLETED="curl -X POST https://example.com/webhook" \
  senko serve
```

## ステータス遷移

```
draft → todo → in_progress → completed
                            → canceled
（アクティブなステータスからcanceledへの遷移は常に可能）
```
