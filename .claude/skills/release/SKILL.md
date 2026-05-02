---
name: release
description: "senkoのリリースを実行する。e2eテスト実行→バージョン自動判定→Cargo.toml更新→コミット→タグ作成→pushを一括で行う。Triggers on \"/release\", \"リリース\", \"リリースして\", \"release\", \"バージョンアップ\", \"新バージョン\" or similar release requests."
argument-hint: "[version（省略時は自動判定）]"
---

# Release — senko リリーススキル

senko の新バージョンをリリースする。e2e テストの成功を確認してから、バージョン更新・コミット・タグ作成・push を実行する。

`Cargo.toml` と `web/package.json` のバージョンは同じ値で同期 bump される。`v*.*.*` タグを push すると `.github/workflows/release.yml` が起動し、Rust バイナリ (3 ターゲット) と並列で `senko-web-${VERSION}.tar.gz` (+ `.sha256`) も build され、同じ GitHub Release に attach される。

## 手順

### Step 1: e2e テストの実行

```bash
bash tests/e2e/run.sh
```

テストが **1つでも失敗したらリリースを中止**し、失敗内容をユーザーに報告して終了する。

### Step 1.5: 起動ログ assertion

`senko serve` が OTel exporter を期待通り初期化することを確認する。0.38.2 で OTel exporter の silent regression を見逃した教訓により導入されたガードレール（Contract #9）。

```bash
bash scripts/release-boot-check.sh
```

このスクリプトは：

1. `cargo build -q --bin senko` でバイナリを準備
2. 期待 env で `senko serve --port 0` を 3 秒だけ起動
   - `OTEL_LOGS_EXPORTER=otlp` / `OTEL_TRACES_EXPORTER=otlp`
   - `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:65535`
   - `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`
   - `SENKO_LOG_FORMAT=json`（Contract #9 で pin）
3. JSON ログを `jq` で構造化検証
   - `traces.status == "enabled"` / `logs.status == "enabled"`
   - `traces.protocol == "http/protobuf"` / `logs.protocol == "http/protobuf"`
   - `"without exporters"` メッセージが出ていないこと
4. 不一致なら **exit 1 でリリースを中止**

スクリプトが失敗したらリリースを中止し、`Cargo.toml` の `opentelemetry-otlp` features（`reqwest-blocking-client`）と `src/bootstrap.rs::init_telemetry` を確認する。

前提: `jq` がホスト環境にインストール済であること。

### Step 2: バージョン番号の決定

引数でバージョンが指定されている場合はそれを使う（`v` プレフィックスは除去して扱う）。

引数がない場合は、前回のタグからの変更内容を分析して自動判定する：

```bash
# 最新のタグを取得
git describe --tags --abbrev=0 2>/dev/null

# 前回タグからの変更を確認
git log <last-tag>..HEAD --oneline
```

**バージョン判定ルール（semver）：**

| 変更の種類 | バージョンアップ |
|---|---|
| 破壊的変更（API変更、CLI引数変更など） | **メジャー** (x.0.0) |
| 新機能追加（feat） | **マイナー** (0.x.0) |
| バグ修正・リファクタ・ドキュメント | **パッチ** (0.0.x) |

コミットメッセージの prefix（feat / fix / refactor / docs 等）を参考に判定する。

**メジャーバージョンアップの場合は AskUserQuestion でユーザーに確認を取る。** 確認なしにメジャーバージョンを上げてはいけない。

### Step 3: Cargo.toml / mise.toml / web/package.json のバージョン更新

`Cargo.toml` の `version = "..."` 行を新しいバージョンに更新する。Edit ツールを使うこと。

あわせて `mise.toml` の `[tools]` セクションにある `"github:hisamekms/senko"` を同じ新バージョンに書き換える。これにより `mise tools` 経由で配布される senko バイナリがリリース後に更新されるようになる。Edit ツールを使うこと。

さらに `web/package.json` の `version` も同じ値に揃える。`web/package-lock.json` も整合を取る必要があるため、Edit ツールではなく `npm version` を使うこと:

```bash
(cd web && npm version <version> --no-git-tag-version)
```

`--no-git-tag-version` で commit・tag は作らず、`web/package.json` と `web/package-lock.json` のバージョン欄だけが書き換わる。このバージョンは Release 時に `mise run web:dist` が `dist/senko-web-${VERSION}.tar.gz` のファイル名生成で参照する。

### Step 4: コミットとタグ作成

```bash
# バージョン更新をコミット（Cargo.lock も同期する）
cargo check --quiet
git add Cargo.toml Cargo.lock mise.toml web/package.json web/package-lock.json
git commit -m "chore: bump version to <version>"

# タグ作成
git tag v<version>
```

### Step 5: push

コミットとタグの両方を push する：

```bash
git push origin HEAD
git push origin v<version>
```

### Step 6: リリースワークフローの完了待ち

`release.yml` は `build` matrix (Rust × 3 ターゲット) と `build-web` ジョブを並列に実行し、両方の成果物が揃ってから `release` ジョブが GitHub Release を作成する。

```bash
# ワークフローの実行IDを取得
gh run list --workflow=release.yml --limit 1

# 完了を待つ
gh run watch <run_id> --exit-status
```

### Step 7: リリースノートの編集

ワークフロー完了後、GitHub リリースに Highlights セクションを追加する。

1. 現在のリリースノートを確認する：

```bash
gh release view v<version> --json body
```

2. `git log <last-tag>..v<version> --oneline` の内容から主な変更点をまとめ、既存のリリースノートの先頭に Highlights セクションを追加する：

```bash
gh release edit v<version> --notes "$(cat <<'EOF'
## Highlights

- **機能名** — 概要説明
- ...

<既存のリリースノート>
EOF
)"
```

### Step 8: 完了報告

リリースページに `senko-web-<version>.tar.gz` (+ `.sha256`) が attach されているかを `gh release view v<version>` で確認する。Rust バイナリ (3 ターゲット分) と web tarball が同じバージョン番号で並んでいれば成功。

リリースが完了したら以下を報告する：

- リリースバージョン（例: v0.2.0）
- リリースページの URL
- 同 Release に attach された web tarball ファイル名 (`senko-web-<version>.tar.gz`)
