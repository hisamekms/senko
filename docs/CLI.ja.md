# CLIリファレンス

[English](CLI.md) | [READMEに戻る](README.ja.md)

## グローバルオプション

```
--output <FORMAT>       json または text（デフォルト: json）
--project-root <PATH>   プロジェクトルート（省略時は自動検出）
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

## `web` – 読み取り専用Webビューアを起動

```bash
localflow web                # 127.0.0.1:3141 でリッスン
localflow web --port 8080    # 127.0.0.1:8080 でリッスン
localflow web --host         # 0.0.0.0:3141 でリッスン（全インターフェース）
```

| オプション | 説明 |
|--------|-------------|
| `--port <PORT>` | リッスンポート（デフォルト: `3141`） |
| `--host` | 全ネットワークインターフェースに公開（`127.0.0.1` の代わりに `0.0.0.0` にバインド） |

`--host` フラグは環境変数 `LOCALFLOW_WEB_HOST` でも設定可能です（`0` と `false` 以外の非空値で有効になります）。

## `watch` – タスクイベントの監視とフック実行

タスクデータベースの変更をポーリングし、イベント検出時に設定されたフックを実行します。

```bash
localflow watch                           # フォアグラウンド（5秒間隔）
localflow watch --interval 10             # ポーリング間隔を変更
localflow watch -d                        # バックグラウンドデーモンとして起動
localflow watch -d --interval 10          # デーモン＋カスタム間隔
localflow watch --log-file /tmp/watch.log # ログファイルパスを指定
localflow watch stop                      # デーモンを停止
localflow watch status                    # デーモンの状態を表示
```

| オプション | 説明 |
|--------|-------------|
| `--interval <SECONDS>` | ポーリング間隔（秒）（デフォルト: `5`） |
| `-d, --daemon` | バックグラウンドデーモンとして実行 |
| `--log-file <PATH>` | ログファイルパス（デーモン時デフォルト: `.localflow/watch.log`） |

| サブコマンド | 説明 |
|------------|-------------|
| `stop` | 実行中のデーモンを停止 |
| `status` | デーモンの状態を表示（実行中/停止、PID、稼働時間） |

### 設定

`.localflow/config.toml` にフックを定義します:

```toml
[hooks]
on_task_added = "echo '新しいタスク' | notify-send -"
on_task_completed = "curl -X POST https://example.com/webhook"
```

| フック | トリガー |
|------|---------|
| `on_task_added` | 新しいタスクがデータベースに追加された |
| `on_task_completed` | タスクが `completed` ステータスに遷移した |

フックは **stdin** でイベントペイロード（JSON）を受け取り、`sh -c` で実行されます。

> イベントは対応するフックが設定されている場合のみ検出されます。

### イベントペイロード

フックのstdinに渡されるJSONオブジェクト:

```json
{
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "task_added",
  "timestamp": "2026-03-24T12:00:00Z",
  "task": { },
  "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5 },
  "ready_count": 2,
  "unblocked_tasks": null
}
```

| フィールド | 型 | 説明 |
|-------|------|-------------|
| `event_id` | string | UUID v4 一意識別子 |
| `event` | string | `"task_added"` または `"task_completed"` |
| `timestamp` | string | ISO 8601（RFC 3339）タイムスタンプ |
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

### ログ出力

デーモン実行時（`-d`）はデフォルトで `.localflow/watch.log` にログを出力します。`--log-file` でパスを変更できます。フォアグラウンドモードでは `--log-file` を指定するとファイルログが有効になります。

ログフォーマット:

```
[2026-03-24T12:00:00Z] [INFO] watch started (interval: 5s)
[2026-03-24T12:00:05Z] [INFO] event detected: task_added task #1 "Write docs"
[2026-03-24T12:00:05Z] [INFO] hook executed: task_added (exit: 0)
[2026-03-24T12:00:10Z] [WARN] hook executed: task_completed (exit: 1)
[2026-03-24T12:00:15Z] [ERROR] hook failed: task_added: No such file or directory
```

| レベル | 説明 |
|-------|-------------|
| `INFO` | 通常の操作（起動、イベント検出、フック実行成功） |
| `WARN` | フックが非ゼロ終了コードを返した |
| `ERROR` | フックの実行に失敗した |

## ステータス遷移

```
draft → todo → in_progress → completed
                            → canceled
（アクティブなステータスからcanceledへの遷移は常に可能）
```
