# Proposal: ファイルベース hooks 登録

Status: **Deferred**（設計のみ、実装は未着手）
Date: 2026-04-18

## 概要

`.senko/<runtime>/<action>/<hook-name>.sh` のようなパスにファイルを配置すると hooks として自動登録される機構の提案。
`command` 以外のオプションはファイル内の専用コメント（mise の `#MISE` directive 相当）で宣言する。

## 動機

- TOML に書くより各 hook がスクリプトとして見通しが良くなる
- スクリプト内でロジックを書きやすい（TOML の `command` は 1 行のシェル文字列に限定）
- 実装と設定が 1 ファイルにまとまり、レビュー時の文脈把握が容易

## mise の参考実装

### 発見ルール
- 発見場所: `mise-tasks/`, `.mise-tasks/`, `mise/tasks/`, `.mise/tasks/`, `.config/mise/tasks/`
- サブディレクトリはネームスペース化（`test/integration` → `test:integration`）
- **実行可能フラグ必須**: `chmod +x` されていないと検出されない

### ディレクティブ構文
```bash
#!/usr/bin/env bash
#MISE description="Build the CLI"
#MISE alias="b"
#MISE sources=["Cargo.toml", "src/**/*.rs"]
#MISE outputs=["target/debug/mycli"]
#MISE env={RUST_BACKTRACE = "1"}
#MISE depends=["lint", "test"]
cargo build
```

- 正規形: `#MISE key=value`（`#` とディレクティブの間にスペース**なし**）
- フォーマッタ対策: `# [MISE] key=value`（`rustfmt` 等がスペースを挿入しても機能する別形式）

### トラストモデル
- mise は `trusted_config_paths` 設定による一括許可のみ。**ファイル個別の trust prompt は無い**
- direnv の `direnv allow` や VSCode workspace trust のような個別承認機構はない

## senko での適用案

### ディレクトリ構造

```
.senko/
  cli/
    hooks/
      task_complete/
        verify.sh
        webhook.sh
      task_publish/
        gate.sh
  workflow/
    task_add/
      hooks/
        validate.sh
    task_complete/
      hooks/
        run_tests.sh
```

- キー構造（[hooks 設定キースキーマ刷新タスク](../../tasks/302) と整合）: `<runtime>.<aggregate>_<action>.hooks.<name>`
- `<name>` はファイル名（拡張子除く）

### コメントディレクティブ

```bash
#!/usr/bin/env bash
#SENKO when=pre
#SENKO mode=sync
#SENKO on_failure=abort
#SENKO env_vars=[{name="WEBHOOK_URL", required=true, description="Webhook 送信先"}]

curl -X POST "$WEBHOOK_URL" -d "$SENKO_TASK_ID"
```

- 正規形: `#SENKO key=value`
- フォーマッタ対策形: `# [SENKO] key=value`
- `command` はファイル自体なので、ディレクティブでは指定不可（上書き禁止）

### TOML との共存

同一 `(runtime, action, name)` が両方に存在する場合のルールは未決。以下の候補:

- TOML 優先（ファイル登録を拡張扱い）
- ファイル優先
- 衝突時エラー（明示的にどちらかを消させる）

## セキュリティレビュー

### ベースライン

現 TOML hooks も `sh -c` で任意コマンドを実行しているため、**capability は増えない**。問題は構造的リスク（発見範囲の広がり、レビュー可視性低下、自動有効化）。

### 識別されたリスク

| # | リスク | 深刻度 | 概要 |
|---|---|---|---|
| 1 | git pull / branch-switch で暗黙に有効化 | HIGH | TOML 1 行変更と違い `.sh` 追加はディレクトリに埋もれる。`git pull` だけで発火設定完了 |
| 2 | サプライチェーン（PR レビュー可視性） | HIGH | `format.sh` のような無害そうな名前で紛れ込ませやすい |
| 3 | 実行ビット切替で休眠 hook 起動 / DoS | MED | `chmod +x` flip する PR。sync+abort で `task_complete` 経路が全員分ブリック可能 |
| 4 | シンボリックリンク / パストラバーサル | MED | `.senko/cli/hooks/task_complete/verify.sh -> /etc/shadow` が read される |
| 5 | コメント構文のオプション注入 | MED | `#SENKO command="rm -rf ~"` のようにコメントから command 自体を上書きできると危険 |

### 推奨緩和策

A. **トラストプロンプト**: 初回または検出ファイル集合/内容ハッシュが変化したとき、`senko hooks trust` の明示実行が必要。信頼情報は `$XDG_STATE_HOME/senko/trust.db` に保存（リポジトリ外）。mise・direnv・VSCode workspace trust のパターンを参考に

B. **シンボリックリンク拒否 + パス正規化**: symlink は walk 段階で除外、canonicalize して `.senko/` 配下に収まることを検証。`..` / NUL / 先頭 `.` を含むファイル名は拒否

C. **コメントパーサの strict whitelist**: `when` / `mode` / `on_failure` / `timeout` 等の列挙・数値のみ許可。`command` / `env_vars` の上書きは不可。未知キーはエラー＋ログ

D. **実行ビット非依存の発見**: 拡張子 + shebang で判定。`chmod +x` PR だけで有効化されないように。必要に応じて `.senko/hooks.lock` のような明示 allowlist と組み合わせる

### 環境別の扱い

環境ごとに信頼境界が異なるため、扱いを分けて検討する:

| 環境 | 設定管理者 | 信頼境界 | git pull による暗黙追加 |
|---|---|---|---|
| CLI | 本人 + チーム（PR 経由） | git | あり |
| workflow | 同上 | 同上 | あり |
| Relay server | 運用者（ops） | デプロイ | 低（CI 経由） |
| Remote server | 同上 | 同上 | 低 |

示唆: **ファイルベース登録は CLI / workflow のみに限定**するのが自然。Relay / Remote はサーバーデプロイ物であり、ファイルベースにする動機が薄く、TOML 管理のまま ops プロセスに載せるほうが運用と整合する。

## 未決事項

1. TOML と file-based の衝突解決ルール
2. トラストプロンプトの具体的 UX（対話 / 環境変数 / --yes オプション）
3. `.senko/hooks.lock` のような明示 allowlist の要否
4. 非 bash スクリプト（Python/Node/PowerShell 等）の扱い — 直接実行 or 無視
5. Windows での実行ビット不在（代替: 拡張子ベース判定）

## 前提タスク

実装に着手する場合、先行して以下が完了している必要がある:

- [hooks 設定キースキーマ刷新](#)（キー構造 `<runtime>.<aggregate>_<action>.hooks.<name>` の確定）
