# localflow

> **Alpha**: 本プロジェクトは開発初期段階です。API、CLIインターフェース、データ形式は予告なく変更される可能性があります。

Claude Code向けのローカルタスク管理ツール。SQLiteベース、依存関係対応、優先度駆動。
Claude Codeスキルとして動作し、AIエージェントによるタスク管理・実行を可能にします。

[English](../README.md)

## 機能

- **タスクライフサイクル**: `draft` → `todo` → `in_progress` → `completed` / `canceled`
- **優先度**: P0（最高）〜 P3（最低）
- **依存関係管理**: 依存タスクが完了するまでブロック
- **次タスク自動選択**: 最高優先度の実行可能タスクを自動選択
- **2種類の出力**: JSON（AI/自動化向け）とテキスト（人間向け）
- **Claude Codeスキル**: `/localflow` スキルによるシームレスなAI駆動タスク管理
- **セットアップ不要**: SQLiteデータベースは初回実行時に自動作成

> **注意**: localflowはプロジェクトルート直下の `.localflow/` にデータを保存します。`.gitignore` に `.localflow/` を追加して、ローカルデータをコミットしないようにしてください。

## インストール

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/localflow/main/install.sh | sh
```

バージョンを指定する場合:

```bash
VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/localflow/main/install.sh | sh
```

デフォルトでは `~/.local/bin` にインストールされます。`LOCALFLOW_INSTALL_DIR` で変更できます。

### ソースからビルド

```bash
cargo build --release
```

バイナリは `target/release/localflow` に生成されます。`PATH` に追加してください。

## Claude Code連携

localflowは主にClaude Codeスキルとして使用します。`skill-install` でセットアップします:

```bash
localflow skill-install
```

プロジェクトに `.claude/skills/localflow/SKILL.md` が生成され、Claude Codeに `/localflow` スキルが登録されます。

### スキルで何ができるか

`/localflow` スキルはClaude Codeに完全なタスク管理ワークフローを提供します:

- 次の実行可能タスクを**自動選択して実行**
- 対話的な計画フェーズ付きで**タスクを追加**（シンプルモードも対応）
- **タスク一覧**の表示と**依存関係グラフ**の可視化
- DoD（完了定義）チェック付きのタスク**完了・キャンセル**
- タスク間の**依存関係管理**

## 典型的な使い方

スキルをインストールしたら、Claude Code内で直接使用できます:

```
/localflow add ユーザー認証の実装
```
対話的な計画フェーズ付きでタスクを追加。Claudeが確認事項を質問し、依存関係を検出し、タスクを確定します。

```
/localflow
```
最高優先度の実行可能タスクを自動選択して作業を開始します。

```
/localflow list
```
全タスクのステータスと優先度を表示します。

```
/localflow graph
```
タスクの依存関係をテキストベースのグラフで可視化します。

```
/localflow complete 3
```
タスク#3を完了としてマーク（DoD項目を先にチェックします）。

## ワークフロー設定

`.localflow/config.toml`の`[workflow]`セクションでタスク完了時の動作を制御できます：

```toml
[workflow]
completion_mode = "pr_then_complete"  # または "merge_then_complete"（デフォルト）
auto_merge = false                    # デフォルト: true
```

| 設定 | 値 | 説明 |
|------|------|------|
| `completion_mode` | `merge_then_complete`（デフォルト）, `pr_then_complete` | `pr_then_complete`の場合、`complete`コマンドが`gh`でPRのマージ状況を検証 |
| `auto_merge` | `true`（デフォルト）, `false` | `false`の場合、PRの承認も検証 |

`localflow config`で現在の設定を表示、`localflow config --init`でテンプレートを生成できます。

## CLIリファレンス

CLIを直接使用する場合は[CLIリファレンス](CLI.ja.md)を参照してください。

## 開発

[開発ガイド](DEVELOPMENT.ja.md)にステータス遷移、データ保存、テストの情報があります。

## ライセンス

MIT
