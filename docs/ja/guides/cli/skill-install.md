# Claude Code skill のインストールと更新

`senko skill-install` はプロジェクトに senko 所有の skill ファイル群を配置し、Claude Code に `/senko` スラッシュコマンドを認識させるコマンドです。

## 初回インストール

プロジェクトルートで:

```bash
senko skill-install
```

生成されるもの:

```
.claude/
├── skills/
│   └── senko/
│       ├── SKILL.md             # skill エントリポイント
│       ├── cli-reference.md     # CLI コマンド早見表
│       ├── scripts/             # skill から呼ぶ補助スクリプト
│       └── workflows/           # 個別ワークフロー手順書
│           ├── auto-select.md
│           ├── add-task.md
│           ├── execute-task.md
│           ├── complete-task.md
│           ├── ...
└── agents/
    └── dod-verifier.md          # DoD 検証用 subagent
```

Claude Code を再起動するか `/help` → skill 一覧で認識を確認してください。

## `/senko` が提供する機能

| スラッシュコマンド | 役割 |
|---|---|
| `/senko` | ready なタスクから 1 件自動選択して実行開始 |
| `/senko start <id>` | 指定 ID のタスクを実行 |
| `/senko add <description>` | 対話的にタスクを整理して追加 |
| `/senko add --simple <description>` | 計画フェーズを省略して追加 |
| `/senko list` | タスク一覧 |
| `/senko graph` | 依存関係を Mermaid グラフで可視化 |
| `/senko complete <id>` | DoD チェックしつつ完了 |
| `/senko cancel <id>` | キャンセル |
| `/senko dod check <task_id> <index>` | DoD 項目を checked にする |
| `/senko dod uncheck <task_id> <index>` | DoD 項目を取り消す |
| `/senko deps add <task_id> --on <dep_id>` | 依存を追加 |
| `/senko deps remove <task_id> --on <dep_id>` | 依存を解除 |
| `/senko deps list <task_id>` | 依存一覧 |
| `/senko config-explain` | 現在の設定値を説明 |
| `/senko config-setup` | 対話的に config.toml を作成・改善 |

Contract 操作は senko CLI を直接叩く (`senko contract add` など)。skill のラッパは現状 Task に特化しています。

## 更新

senko のバージョンを上げた後は skill を更新:

```bash
senko skill-install
```

- 既存ファイルと内容が同一ならスキップ (`is up to date` と表示)
- 内容が異なる場合はファイルごとに上書き確認プロンプト (`--yes` で全て承諾)
- `--force` で senko 所有ディレクトリを先に削除してからクリーンインストール

## 配置先の変更

既定は `.claude/` 配下ですが、`--output-dir` で変更可能:

```bash
senko skill-install --output-dir /custom/path
```

`--output-dir` 指定時も `workflows/` `scripts/` などのディレクトリ構造は保持されます（`SKILL.md` 自体が `${CLAUDE_SKILL_DIR}/workflows/...` のような相対パスで参照しているため）。Claude Code の規約に従って認識させるには、最終的に `.claude/skills/<name>/SKILL.md` という階層に配置する必要があります。

## プロジェクトの workflow 設定との関係

skill は実行時に `senko config --output json` を叩き、`[workflow.*]` の instructions / prompt を読み込んでエージェント指示に混ぜます。

つまり:

1. `.senko/config.toml` の `[workflow.*]` を変更
2. **skill の再インストールは不要**。次回の `/senko` 実行時に最新の設定が読まれる

ただし SKILL.md 自体の骨格を更新 (= senko バイナリを新バージョンにする) した場合は `senko skill-install` で再生成してください。

## 複数プロジェクトで使う

senko skill はプロジェクトローカル (`.claude/`) に配置されるので、プロジェクトごとに別の `[workflow.*]` 設定が使えます。全プロジェクト共通の設定は `~/.config/senko/config.toml` に書くと、project 個別設定よりも低優先度で適用されます。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| `/senko` が Claude Code に出てこない | `.claude/skills/senko/SKILL.md` が存在するか、Claude Code を再起動 |
| skill が古い挙動をする | `senko skill-install --force` で再配置 |
| workflow 設定が反映されない | `senko config` で `[workflow.*]` が期待通りマージされているか確認。`senko doctor` も実行 |

## 次に読むもの

- workflow 設定の概念 → [イベントドリブンなワークフロー](../../explanation/event-driven-workflow.md)
- `[workflow.*]` の TOML → [`[workflow.*]` 設定](../../reference/config/workflow.md)
- 実例集 → [Workflow stage の実例](workflow-stages.md)
