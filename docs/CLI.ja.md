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
```

## `complete <id>` – タスク完了

```bash
localflow complete 1
```

未チェックのDoD項目がある場合は失敗します。先に `dod check` でマークしてください。

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

## ステータス遷移

```
draft → todo → in_progress → completed
                            → canceled
（アクティブなステータスからcanceledへの遷移は常に可能）
```
