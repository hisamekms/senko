# CLIリファレンス

[English](CLI.md) | [READMEに戻る](README.ja.md)

## グローバルオプション

```
--output <FORMAT>       json または text（デフォルト: json）
--project-root <PATH>   プロジェクトルート（省略時は自動検出）
--config <PATH>         設定ファイルのパス（環境変数: LOCALFLOW_CONFIG、デフォルト: .localflow/config.toml）
--dry-run               実行せずに結果を表示（状態変更コマンドのみ）
```

> **注意**: `--output` と `--dry-run` はグローバルフラグです。サブコマンドの**前**に配置してください: `localflow --output text list`

## `add` – タスク作成

```bash
localflow add --title "ドキュメント作成" --priority p0
localflow add --title "バグ修正" \
  --background "ユーザーから500エラーの報告" \
  --definition-of-done "ログに500エラーなし" \
  --in-scope "エラーハンドラ" \
  --out-of-scope "リファクタリング" \
  --tag backend --tag urgent
```

新規タスクは `draft` ステータスで作成されます。デフォルト優先度は `p2`。

## `list` – タスク一覧

```bash
localflow list                    # 全タスク
localflow list --status todo      # ステータスで絞り込み
localflow list --ready            # 依存解決済みのtodoタスク
localflow list --tag backend      # タグで絞り込み
```

CLIフラグのステータス値はスネークケース: `todo`, `in_progress`, `completed`, `canceled`, `draft`

## `get <id>` – タスク詳細

```bash
localflow get 1
```

> `get` はJSON出力のみ（`--output text` 非対応）。

## `next` – 次のタスクを開始

依存タスクがすべて完了済みの最高優先度 `todo` タスクを選択し、`in_progress` に変更します。

```bash
localflow next
localflow next --session-id "session-abc"
```

選択順序: 優先度（P0優先）→ 作成日時 → ID

## `edit <id>` – タスク編集

```bash
# スカラーフィールド
localflow edit 1 --title "新しいタイトル"
localflow edit 1 --status todo
localflow edit 1 --priority p0

# 配列フィールド（タグ、完了定義、スコープ）
localflow edit 1 --add-tag "urgent"
localflow edit 1 --remove-tag "old"
localflow edit 1 --set-tags "a" "b"         # 全置換

# 完了定義（Definition of Done）
localflow edit 1 --add-definition-of-done "ユニットテストを書く"

# PR URL
localflow edit 1 --pr-url "https://github.com/org/repo/pull/42"
localflow edit 1 --clear-pr-url
```

## `complete <id>` – タスク完了

```bash
localflow complete 1
localflow complete 1 --skip-pr-check    # PR検証をスキップ
```

未チェックのDoD項目がある場合は失敗します。先に `dod check` でマークしてください。

`completion_mode = "pr_then_complete"` 設定時は、PRがマージ済みであることも検証します（`auto_merge = false` の場合は承認も確認）。`--skip-pr-check` で検証をスキップできます。

## `cancel <id>` – タスクキャンセル

```bash
localflow cancel 1 --reason "スコープ外"
```

## `dod` – 完了定義（DoD）の管理

```bash
localflow dod check <task_id> <index>      # DoD項目をチェック（1始まり）
localflow dod uncheck <task_id> <index>    # DoD項目のチェックを外す
```

## `deps` – 依存関係管理

```bash
localflow deps add 5 --on 3        # タスク5がタスク3に依存
localflow deps remove 5 --on 3     # 依存を削除
localflow deps set 5 --on 1 2 3    # 依存を一括設定
localflow deps list 5              # タスク5の依存一覧
```

## `config` – 設定の表示・初期化

```bash
localflow config              # 現在の設定を表示（JSON）
localflow --output text config # 現在の設定を表示（テキスト）
localflow config --init       # テンプレート .localflow/config.toml を生成
```

現在の設定値（未設定項目はデフォルト値）を表示します。`--init` でコメント付きテンプレートファイルを生成します。

## `skill-install` – Claude Code連携

```bash
localflow skill-install
```

Claude Code連携用のスキル定義を `.claude/skills/localflow/` に生成します。

## `serve` – JSON APIサーバーを起動

```bash
localflow serve                # 127.0.0.1:3142 でリッスン
localflow serve --port 8080    # 127.0.0.1:8080 でリッスン
localflow serve --host 0.0.0.0 # 0.0.0.0:3142 でリッスン（全インターフェース）
```

| オプション | 説明 |
|--------|-------------|
| `--port <PORT>` | リッスンポート（環境変数: `LOCALFLOW_PORT`、デフォルト: `3142`） |
| `--host <ADDR>` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`）（環境変数: `LOCALFLOW_HOST`、デフォルト: `127.0.0.1`） |

`/api/v1/...` 配下で全タスク操作（CRUD、ステータス遷移、依存関係、DoD、設定、統計）をJSON REST APIとして提供します。CLIと同様にhooksが発火します。

## `web` – 読み取り専用Webビューアを起動

```bash
localflow web                # 127.0.0.1:3141 でリッスン
localflow web --port 8080    # 127.0.0.1:8080 でリッスン
localflow web --host 0.0.0.0 # 0.0.0.0:3141 でリッスン（全インターフェース）
```

| オプション | 説明 |
|--------|-------------|
| `--port <PORT>` | リッスンポート（環境変数: `LOCALFLOW_PORT`、デフォルト: `3141`） |
| `--host <ADDR>` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`）（環境変数: `LOCALFLOW_HOST`、デフォルト: `127.0.0.1`） |

## フック – タスク状態変更時の自動アクション

フックはCLIコマンドがタスク状態を変更した際に自動実行されるシェルコマンドです。デーモン不要で、fire-and-forget（発火後即座に制御を返す）方式で子プロセスとして実行されるため、CLIをブロックしません。

### 設定

`.localflow/config.toml` にフックを定義します:

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
| `on_task_added` | `localflow add` で新しいタスクを作成 |
| `on_task_ready` | `localflow ready` でタスクを draft から todo に遷移 |
| `on_task_started` | `localflow start` または `localflow next` でタスクを開始 |
| `on_task_completed` | `localflow complete` でタスクを完了 |
| `on_task_canceled` | `localflow cancel` でタスクをキャンセル |

フックは **stdin** でイベントペイロード（JSON）を受け取り、`sh -c` で実行されます。

### イベントペイロード

フックのstdinに渡されるJSONオブジェクト:

```json
{
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "task_completed",
  "timestamp": "2026-03-24T12:00:00Z",
  "from_status": "in_progress",
  "task": { },
  "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5 },
  "ready_count": 2,
  "unblocked_tasks": [{ "id": 3, "title": "次のタスク", "priority": "P1", "metadata": null }]
}
```

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `event_id` | string | UUID v4 一意識別子 |
| `event` | string | イベント名（例: `"task_added"`, `"task_completed"`） |
| `timestamp` | string | ISO 8601（RFC 3339）タイムスタンプ |
| `from_status` | string \| null | 遷移前のステータス |
| `task` | object | タスクオブジェクト全体（`localflow get` と同じスキーマ） |
| `stats` | object | ステータス別タスク数（`{"todo": 3, "completed": 5, ...}`） |
| `ready_count` | integer | 依存解決済みの `todo` タスク数 |
| `unblocked_tasks` | array \| null | このイベントで新たにブロック解除されたタスク（`task_completed` のみ） |

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
| `LOCALFLOW_PORT` | `web` / `serve` コマンドのポート | `3141`（web）/ `3142`（serve） |
| `LOCALFLOW_HOST` | バインドアドレス（例: `0.0.0.0`, `192.168.1.5`） | `127.0.0.1` |
| `LOCALFLOW_PROJECT_ROOT` | プロジェクトルートディレクトリ | 自動検出 |
| `LOCALFLOW_CONFIG` | 設定ファイルのパス | `.localflow/config.toml` |

### ワークフロー

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `LOCALFLOW_COMPLETION_MODE` | `merge_then_complete` または `pr_then_complete` | `merge_then_complete` |
| `LOCALFLOW_AUTO_MERGE` | `true` または `false` | `true` |

### バックエンド

| 変数 | 説明 | デフォルト |
|------|------|----------|
| `LOCALFLOW_API_URL` | APIサーバーURL（設定するとSQLiteの代わりにHTTPバックエンドを使用） | _（未設定 = SQLite）_ |
| `LOCALFLOW_HOOK_MODE` | `server`、`client`、または `both` | `server` |

### フック

| 変数 | 説明 |
|------|------|
| `LOCALFLOW_HOOK_ON_TASK_ADDED` | タスク作成時に実行するシェルコマンド |
| `LOCALFLOW_HOOK_ON_TASK_READY` | タスクがready時に実行するシェルコマンド |
| `LOCALFLOW_HOOK_ON_TASK_STARTED` | タスク開始時に実行するシェルコマンド |
| `LOCALFLOW_HOOK_ON_TASK_COMPLETED` | タスク完了時に実行するシェルコマンド |
| `LOCALFLOW_HOOK_ON_TASK_CANCELED` | タスクキャンセル時に実行するシェルコマンド |

フック環境変数は `config.toml` の `[hooks]` セクションの設定をオーバーライドします。

### 例: Dockerデプロイ

```bash
docker run -e LOCALFLOW_PORT=8080 \
  -e LOCALFLOW_HOST=0.0.0.0 \
  -e LOCALFLOW_HOOK_ON_TASK_COMPLETED="curl -X POST https://example.com/webhook" \
  localflow serve
```

## ステータス遷移

```
draft → todo → in_progress → completed
                            → canceled
（アクティブなステータスからcanceledへの遷移は常に可能）
```
