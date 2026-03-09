# localflow

ローカル専用のタスク管理CLI。個人開発・単一エージェント向け。
SQLiteベース、依存関係対応、優先度駆動。

[English](../README.md)

## 機能

- **タスクライフサイクル**: `draft` → `todo` → `in_progress` → `completed` / `canceled`
- **優先度**: P0（最高）〜 P3（最低）
- **依存関係管理**: 依存タスクが完了するまでブロック
- **次タスク自動選択**: 最高優先度の実行可能タスクを自動選択
- **2種類の出力**: JSON（AI/自動化向け）とテキスト（人間向け）
- **Claude Code連携**: `skill-install` でスキル設定を生成
- **セットアップ不要**: SQLiteデータベースは初回実行時に自動作成

## インストール

### ソースからビルド

```bash
cargo build --release
```

バイナリは `target/release/localflow` に生成されます。

### Claude Code連携

```bash
localflow skill-install
```

Claude Codeスキル連携用の `SKILL.md` を生成します。

## クイックスタート

```bash
# タスク作成
localflow add --title "認証APIの実装" --priority p1

# タスク一覧
localflow list

# 次のタスクを開始
localflow next

# タスク完了
localflow complete 1
```

## コマンド一覧

### グローバルオプション

```
--output <FORMAT>       json または text（デフォルト: text）
--project-root <PATH>   プロジェクトルート（省略時は自動検出）
```

### `add` – タスク作成

```bash
localflow add --title "ドキュメント作成" --priority p0
localflow add --title "バグ修正" \
  --background "ユーザーから500エラーの報告" \
  --definition-of-done "ログに500エラーなし" \
  --in-scope "エラーハンドラ" \
  --out-of-scope "リファクタリング" \
  --tag backend --tag urgent
```

### `list` – タスク一覧

```bash
localflow list                    # 全タスク
localflow list --status todo      # ステータスで絞り込み
localflow list --ready            # 依存解決済みのtodoタスク
localflow list --tag backend      # タグで絞り込み
```

### `get <id>` – タスク詳細

```bash
localflow get 1
localflow get 1 --output json
```

### `next` – 次のタスクを開始

依存タスクがすべて完了済みの最高優先度 `todo` タスクを選択し、`in_progress` に変更します。

```bash
localflow next
localflow next --session-id "session-abc"
```

選択順序: 優先度（P0優先）→ 作成日時 → ID

### `edit <id>` – タスク編集

```bash
# スカラーフィールド
localflow edit 1 --title "新しいタイトル"
localflow edit 1 --status todo
localflow edit 1 --priority p0

# 配列フィールド（タグ、完了定義、スコープ）
localflow edit 1 --add-tag "urgent"
localflow edit 1 --remove-tag "old"
localflow edit 1 --set-tags "a" "b"         # 全置換
```

### `complete <id>` – タスク完了

```bash
localflow complete 1
```

### `cancel <id>` – タスクキャンセル

```bash
localflow cancel 1 --reason "スコープ外"
```

### `deps` – 依存関係管理

```bash
localflow deps add 5 --on 3        # タスク5がタスク3に依存
localflow deps remove 5 --on 3     # 依存を削除
localflow deps set 5 --on 1 2 3    # 依存を一括設定
localflow deps list 5              # タスク5の依存一覧
```

### `skill-install` – Claude Code連携

```bash
localflow skill-install
```

## ステータス遷移

```
draft → todo → in_progress → completed
                    ↓
                 canceled
```

- `draft` → `todo` → `in_progress` → `completed`: 前方遷移のみ
- アクティブな状態 → `canceled`: 常に可能
- 後方遷移・自己遷移は不可

## データ保存

データベースは `<プロジェクトルート>/.localflow/data.db` に自動作成されます。

プロジェクトルートは `.localflow/`、`.git/` の存在で自動検出されます（カレントディレクトリにフォールバック）。

## テスト

```bash
cargo test                    # ユニットテスト
bash tests/e2e/run.sh         # E2Eテスト
```

## ライセンス

MIT
