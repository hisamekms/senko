# テスト

senko は 2 層のテストを持ちます:

1. **Unit test** — Rust の `#[test]` (ドメインロジック・value object の単体)
2. **E2E test** — `tests/e2e/*.sh` (CLI から実行して挙動を確認)

## 実行

```bash
mise test          # unit test + doc test
mise test-e2e      # end-to-end
```

> **ルール**: 直接 `cargo test` / `bash tests/e2e/run.sh` を使わず、必ず `mise` タスク経由で。mise が環境変数・PostgreSQL embedded 等をセットアップする。

### 一部だけ走らせたい

mise タスクに追加引数を渡せる:

```bash
mise test task::tests::                    # 特定モジュール
mise test -- --nocapture                   # println! を出す
mise test-e2e test_contract_crud.sh        # 個別の e2e ファイル
```

## Unit test の書き方

各モジュールの `mod tests` 内で書く。ドメイン層の value object や status transition はここが主戦場:

```rust
// src/domain/task.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_to_todo_is_allowed() {
        let t = Task::new("x");
        let t = t.ready().unwrap();
        assert_eq!(t.status, Status::Todo);
    }

    #[test]
    fn cannot_complete_from_draft() {
        let t = Task::new("x");
        assert!(t.complete().is_err());
    }
}
```

- **domain**: 純粋な状態遷移・バリデーション
- **application**: port の mock / stub を使ってサービスを叩く
- **infra**: SQLite 相手の integration test (一時 DB を tempfile で作る)

## E2E test

シェルスクリプトで **実バイナリ** を叩いて挙動を確認。`tests/e2e/helpers.sh` に共通関数あり。

### 構造

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_fresh_project   # tempdir に .senko を切る
trap cleanup EXIT

senko task add --title hello
out=$(senko task list)
assert_json "$out" '.[] | select(.title=="hello")'
```

### 主要な helper

| 関数 | 用途 |
|---|---|
| `setup_fresh_project` | 新しい tempdir + project root 設定 |
| `start_serve` / `stop_serve` | バックグラウンドで `senko serve` 起動 |
| `assert_eq` / `assert_json` | 結果比較 |
| `cleanup` | tempdir 削除 |

### PostgreSQL を相手にする e2e

```bash
mise test-e2e -- --postgres    # postgresql_embedded を使って Postgres 相手にも走る
```

`postgresql_embedded` dev-dependency が JVM を落としてきて一時 Postgres を立てます。時間がかかるのでローカルでは選択的に。

### HTTP backend を相手にする e2e

```bash
# 個別の test_http_* は内部で serve を起動→CLI から HTTP 経由でアクセス
mise test-e2e test_http_backend.sh
mise test-e2e test_serve_api.sh
mise test-e2e test_http_hooks.sh
```

### 認証系の e2e

```bash
test_api_keys.sh
test_auth_session.sh
test_auth_token.sh
test_token_relay.sh
test_trusted_headers.sh
```

## CI

GitHub Actions で:

- `cargo fmt --check`
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo test --all-features`
- `mise test-e2e` (SQLite + HTTP)

PR を作ると自動実行。失敗した場合はローカルで再現して修正。

## カバレッジ目標

- domain 層: 主要パス 100%
- application 層: 正常系 + 主要エラー系
- infra 層: 代表的なクエリ (list/filter/transaction)
- e2e: **追加機能は必ず e2e でカバー** (ユーザーからの挙動観察点なので)

## テストを書くガイドライン

- **1 テスト 1 観察** — 1 つの `#[test]` で複数の観点を一度に検査しない
- **e2e は "人間が CLI でやる手順" を模倣**。内部実装を触らず、CLI 出力だけで判定
- **時系列/日付に依存するテストは要注意** — `chrono::Utc::now()` を直接使わず、テストに `fn now() -> DateTime<Utc>` を差し替えられる設計にする
- **失敗時の diff が読めるようにする** — `assert_eq!(got, expected)` で got/expected の順を守る
