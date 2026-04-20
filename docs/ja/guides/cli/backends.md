# CLI backend を切り替える

`senko` CLI は実行時に **どこに task データを保存するか** を 3 択から決定します。

| Backend | 設定方法 | 用途 |
|---|---|---|
| SQLite (ローカル) | 既定 | 個人開発、プロジェクト単位の完結 |
| PostgreSQL (ローカル/リモート DB) | `SENKO_POSTGRES_URL` 等 | サーバ構成で PostgreSQL を直接読みたい CLI (稀) |
| HTTP (remote server) | `[cli.remote]` | チームサーバに接続する CLI |

## 決定優先順位

bootstrap は以下の順で backend を選びます:

1. `[cli.remote] url` or `SENKO_CLI_REMOTE_URL` が設定されている → **HTTP backend**
2. `[backend.postgres] url` or `SENKO_POSTGRES_URL` が設定されている (feature 有効時) → **PostgreSQL**
3. それ以外 → **SQLite**

## SQLite (既定)

```
データの場所: $XDG_DATA_HOME/senko/projects/<dir-name>/data.db
           (= 通常 ~/.local/share/senko/projects/<dir-name>/data.db)
```

`<dir-name>` は project root のディレクトリ名。同名プロジェクトが衝突する場合は `db_path` を明示すること。

上書きしたい場合:

```toml
[backend.sqlite]
db_path = "/custom/location/data.db"
```

or CLI/env:

```bash
senko --db-path /custom/data.db task list
SENKO_DB_PATH=/custom/data.db senko task list
```

過去バージョンで `<project_root>/.senko/data.db` を使っていた場合、初回起動で XDG 側に自動マイグレーションされます (元ファイルは検証用に残ります)。

## PostgreSQL (CLI 直接接続、稀)

本来 PostgreSQL は `senko serve` サーバの backend として使う想定ですが、CLI から直接接続することも可能です (開発・移行用途):

```bash
cargo build --release --features postgres
export SENKO_POSTGRES_URL="postgres://user:pass@localhost/senko"
senko task list
```

- 起動時に未適用のマイグレーションを自動実行
- 複数 CLI からの同時書き込みは Postgres のトランザクションで防がれるが、CLI 直接接続は推奨しない。通常はサーバ経由で (HTTP backend)

## HTTP (remote server)

チームサーバに繋ぐ最も一般的な構成:

```toml
# .senko/config.toml
[cli.remote]
url = "https://senko.example.com"
token = "sk_..."
```

または env:

```bash
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="sk_..."
senko task list
```

remote 経由では **ローカル DB は一切触られません**。すべての操作が HTTP 経由でサーバに送られる。

### token を keychain に預ける (OIDC)

サーバが OIDC を有効化している場合:

```bash
senko auth login
```

でブラウザログイン後、OS keychain に token が保存されます。以降は config に token を書かなくて OK:

```toml
[cli.remote]
url = "https://senko.example.com"
# token は keychain から自動取得
```

## 一時的に別 backend を使う

同じリポジトリで一回だけ切り替えたい:

```bash
# remote 設定を一時的に無効化してローカル DB へ
SENKO_CLI_REMOTE_URL= senko task list

# 別 DB ファイルを触る
senko --db-path /tmp/scratch.db task list

# 別 Postgres へ (dev migration 等)
SENKO_POSTGRES_URL=postgres://... senko task list
```

## backend の確認

```bash
senko config
```

の出力で `cli.remote.url` / `backend.sqlite.db_path` / `backend.postgres.url` のいずれかに値が入っているかを確認。

## データ移行 (SQLite → Postgres 等)

**現時点で backend 間のデータ移行手段は用意されていません。** 公式の移行コマンドも、ロードマップ上の提供予定もありません。既存データを別 backend に移す必要がある場合は、運用側で独自にダンプ/リストア手順を用意する前提で計画してください。
