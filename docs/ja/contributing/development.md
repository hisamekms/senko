# 開発環境セットアップ

senko をソースからビルドし、開発する時のセットアップ手順。

## 必須要件

- Rust (edition 2024 対応版)
- `mise` — プロジェクトのツールバージョン管理に使用 (`mise.toml`)
- git
- (PostgreSQL backend を触るなら) Docker or ローカル Postgres

## 初回セットアップ

```bash
git clone https://github.com/hisamekms/senko.git
cd senko

# mise でツールチェーンを揃える
mise install

# ビルド
cargo build

# 実行
cargo run -- task list
```

## feature flags

```toml
# Cargo.toml
[features]
aws-secrets = ["dep:aws-sdk-secretsmanager", "dep:aws-config"]
postgres    = ["dep:sqlx"]
```

- **`aws-secrets`**: AWS Secrets Manager 統合 (`_arn` 指定の解決)
- **`postgres`**: PostgreSQL backend

フルセットでビルド:

```bash
cargo build --all-features
```

## ディレクトリ構造

```
src/
├── domain/        ドメインモデル (依存なし)
├── application/   ユースケース + port trait
│   └── port/
├── infra/         port 実装 (sqlite / postgres / http / hook / auth)
│   └── postgres/migrations/
├── presentation/  cli / api / web
└── bootstrap.rs   依存関係の組み立て

tests/
└── e2e/           シェルスクリプトベースの E2E テスト
```

設計原則: [4 層アーキテクチャ](architecture.md)

## 作業の進め方

### worktree で作業する

このプロジェクトでは **mainブランチで直接編集しない** ルール:

```bash
# worktree 作成
./scripts/bin/wth add my-feature

# 作業ディレクトリへ
cd worktrees/my-feature
```

詳細: [worktree.md](worktree.md)

### ビルド & テスト

```bash
mise test           # unit + doc test
mise test-e2e       # end-to-end (bash スクリプト)
```

詳細: [testing.md](testing.md)

### アーキテクチャチェック

レイヤー間の依存違反を検出:

```
/arch-review
```

(Claude Code skill)。手動で実行する場合は `docs/arch-review/` の最新ファイルを参照。

## 実装・修正フロー

1. worktree を作って作業開始
2. 変更を加える (domain → application → infra → presentation の順で上から下に実装するのが基本)
3. 対応する unit test を追加/更新
4. e2e test で一通り確認
5. PR 作成 (`/review-pr` / `/security-review` skill も使える)

## IDE / エディタ

- **rust-analyzer**: 設定ファイル特になし
- **clippy**: `cargo clippy --all-features --all-targets -- -D warnings`
- **rustfmt**: `cargo fmt` (設定は既定)

CI で clippy と rustfmt が実行されるので、PR 前にローカルでも必ず通してください。

## マイグレーション追加

SQLite:

```
src/infra/sqlite/mod.rs の MIGRATIONS 定数に Migration エントリを追加
```

PostgreSQL:

```
src/infra/postgres/migrations/ に タイムスタンプ_名前.sql を追加
```

両方同時に追加し、同じスキーマを表現すること。`schema_migrations` テーブルで version 管理されるため、番号は単調増加。

## 依存関係の更新

- **Cargo / GitHub Actions**: Dependabot (`.github/dependabot.yml`)
- **mise ツール**: Renovate (`renovate.json5`)

Renovate はリリースから 7 日待って PR を開きます。自動マージは両方無効 — 手動レビュー必須。

## デバッグ

```bash
# ログ出力を詳細に
cargo run -- --log-dir /tmp/senko-logs task list
RUST_LOG=debug cargo run -- task list

# 一時 DB で試す
cargo run -- --db-path /tmp/test.db task add --title hello

# Postgres 相手にローカル DB で試す (postgres feature)
docker run -d --name senko-pg -e POSTGRES_PASSWORD=pw -p 5432:5432 postgres:16
SENKO_POSTGRES_URL="postgres://postgres:pw@127.0.0.1:5432/postgres" \
  cargo run --features postgres -- task list
```

## リリース

`/release` skill で一括実行 (e2e → バージョン判定 → Cargo.toml 更新 → commit → tag → push)。詳細: [releasing.md](releasing.md)
