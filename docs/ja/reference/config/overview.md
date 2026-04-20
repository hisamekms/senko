# 設定ファイル概論

senko の設定は TOML で、複数ファイルをマージしてから **実行中 runtime に応じた section** だけが有効化されます。

## ファイル配置と優先順位

値の解決順 (上が強い):

1. **CLI フラグ** (`--config <path>`、`--port`、`--host` …)
2. **環境変数** (`SENKO_*`)
3. **Local 設定** `.senko/config.local.toml` — git 管理外、開発者ごとの上書き
4. **Project 設定** `.senko/config.toml` — git 管理、チーム共有
5. **User 設定** `~/.config/senko/config.toml` — 全プロジェクト共通
6. **組み込みデフォルト**

同じキーが複数の層にあれば、**スカラーは上位が勝ち、テーブル (hook 等) は名前単位でマージ** されます。

テンプレート生成:

```bash
senko config --init > .senko/config.toml
```

## トップレベル section 一覧

| Section | 効くタイミング | 詳細ページ |
|---|---|---|
| `[project]` / `[user]` / `[log]` / `[web]` | 常時 (`[web]` は `senko web` 時のみ) | [`[project]` / `[user]` / `[log]` / `[web]` 設定](common.md) |
| `[backend.sqlite]` / `[backend.postgres]` | direct backend 利用時 | [`[server.*]` / `[backend.*]` / `[server.auth.*]` 設定](server-remote.md) |
| `[cli]` / `[cli.remote]` | ローカル CLI (= `serve` 以外) | [`[cli.*]` 設定](cli.md) |
| `[server]` / `[server.auth.*]` / `[server.remote]` | `senko serve` direct mode | [`[server.*]` / `[backend.*]` / `[server.auth.*]` 設定](server-remote.md) |
| `[server.relay]` | `senko serve` relay mode (`url` 設定で自動切替) | [`[server.relay.*]` 設定](server-relay.md) |
| `[workflow]` / `[workflow.<stage>]` | skill 消費 (全 runtime で読まれ得る) | [`[workflow.*]` 設定](workflow.md) |

## Runtime フィルタ

**実行中の runtime にマッチしない section の hook は発火しません**。例えば `senko task add` (= `cli` runtime) 中は `[server.remote.*]` の hook は読まれない。

起動時に mismatch な section が見つかると warning が 1 回出力されます:

```
hooks configured under runtime sections that do not match the active runtime; they will not fire
```

runtime の選び方は [Runtime の使い分け](../../explanation/runtimes.md) 参照。

## Secrets の扱い

AWS Secrets Manager を使う場合 (`aws-secrets` feature 有効ビルド):

| 直接値 | ARN 版 |
|---|---|
| `SENKO_AUTH_API_KEY_MASTER_KEY` | `SENKO_AUTH_API_KEY_MASTER_KEY_ARN` |
| `[server.auth.api_key] master_key` | `[server.auth.api_key] master_key_arn` |
| `[backend.postgres] url` | `[backend.postgres] url_arn` or `rds_secrets_arn` |

ARN 指定時は起動時に解決され、メモリ上では平文になります。ログにも出ません (zeroize 済)。

## Config 確認

```bash
senko config                    # マージ済み現在値を JSON で
senko config --output text      # 人間向け表示
senko doctor                    # 設定 + hook + マイグレーション健全性
```

## よくある間違い

- **`[hooks.*]` という古い形式** — v1 では削除。`[cli.task_*]` `[server.*]` に置き換える必要あり (legacy config の migration は自動ではない)
- **`pre_hooks` / `post_hooks` 配列** — 削除。`hooks.<name>.when = "pre" | "post"` に置き換え
- **`on_no_eligible_task` イベント** — 削除。`[cli.task_select.hooks.<name>]` + `on_result = "none"` で置き換え
- **runtime を書き忘れる** — `[task_add.hooks.*]` のように runtime を省くと、どの section にも属さないので発火しない
