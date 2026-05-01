# `[project]` / `[user]` / `[log]` 設定

runtime に関わらず常時読まれる共通 section。

## `[project]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `name` | string | `null` | 操作対象プロジェクト名。未設定なら `default` (id=1) |

env override: `SENKO_PROJECT`
CLI override: `--project <name>`

リモート接続 (`[cli.remote]` 設定あり) の場合、サーバ上に存在するプロジェクト名である必要があります。無ければ 404。

## `[user]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `name` | string | `null` | 操作ユーザ名。未設定なら `default` (id=1) |

env override: `SENKO_USER`
CLI override: `--user <name>`

`task add --assignee-user-id self` や hook envelope の `user` フィールドの解決に使われます。

## `[log]`

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `dir` | string | `$XDG_STATE_HOME/senko` | ログファイルディレクトリ |
| `level` | string | `"info"` | `trace` / `debug` / `info` / `warn` / `error` |
| `format` | string | `"json"` | `"json"` or `"pretty"` |
| `hook_output` | string | `"file"` | `"file"` / `"stdout"` / `"both"` — hook の stdout/stderr 出力先 |

env override: `SENKO_LOG_DIR` (`dir` のみ)
CLI override: `--log-dir <path>`

`hook_output`:

- `file`: hook の出力はログファイルにのみ書かれる (コンソールには出ない)
- `stdout`: CLI のコンソールにそのまま出す
- `both`: 両方

デバッグ時は `--log-dir` + `[log] level = "debug"` が便利。
