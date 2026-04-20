# リリース手順

senko のリリースは GitHub Actions で自動化されています。バージョンタグを push すればビルドと GitHub Release 公開まで走ります。

## 通常フロー (`/release` skill 推奨)

Claude Code で:

```
/release
```

skill が自動で以下を実行:

1. `mise test-e2e` を実行
2. コミット差分からバージョンアップ種別を判定 (patch / minor / major)
3. `Cargo.toml` の version を更新
4. `chore: bump version to X.Y.Z` で commit
5. `vX.Y.Z` tag を作成
6. push

push をトリガに `.github/workflows/release.yml` が起動してビルド + Release 公開。

## 手動で出す場合

```bash
# 1. e2e が通ることを確認
mise test-e2e

# 2. バージョンを上げる
vim Cargo.toml     # version = "1.0.0" に更新
cargo build         # Cargo.lock を更新

# 3. commit
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 1.0.0"

# 4. タグを付けて push
git tag v1.0.0
git push origin main
git push origin v1.0.0
```

> `v` prefix 必須 (`1.0.0` ではなく `v1.0.0`)。

## ビルド対象プラットフォーム

| Target | OS | Arch |
|---|---|---|
| `aarch64-apple-darwin` | macOS | ARM64 (Apple Silicon) |
| `aarch64-unknown-linux-musl` | Linux | ARM64 |
| `x86_64-unknown-linux-musl` | Linux | x86_64 |

Intel macOS / Windows は現状サポート対象外。

## 成果物

各 Release に:

```
senko-vX.Y.Z-<target>.tar.gz
senko-vX.Y.Z-<target>.tar.gz.sha256
```

- tar.gz 内に `senko` バイナリが 1 つ
- `.sha256` で改竄検知可

## インストール (ユーザ側)

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
# 特定バージョン
VERSION=v1.0.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
```

## バージョニング方針

v1 リリース以降は **SemVer** に従います:

- **MAJOR**: 破壊的変更 (CLI 削除、設定キー削除、DB スキーマの非互換変更)
- **MINOR**: 後方互換の機能追加 (新 CLI / 新 config キー / 新 API エンドポイント)
- **PATCH**: bug fix、依存更新、ドキュメント

0.x.y の間は MINOR を破壊的変更に充てていましたが、**v1.0.0 以降は破壊的変更は MAJOR のみ**。

## CHANGELOG

GitHub Release の自動生成 notes を使用。PR title を使うので:

- `feat: ...` → 新機能
- `fix: ...` → 修正
- `docs: ...` → ドキュメント
- `chore: ...` → 雑多
- `refactor: ...` → リファクタ

Conventional Commits に寄せると分類が綺麗になります。

## リリース前チェックリスト

- [ ] `mise test` 通る
- [ ] `mise test-e2e` 通る
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` 通る
- [ ] `cargo fmt --check` 通る
- [ ] 破壊的変更がある場合、ドキュメント (特に migration) を更新
- [ ] (MAJOR bump) リリース前に RC を 1 週間以上動かす

## 検証 (ユーザ側視点)

```bash
# SHA-256 検証
sha256sum -c senko-v1.0.0-x86_64-unknown-linux-musl.tar.gz.sha256

# 起動確認
./senko --version
./senko task list
```

## ロールバック

公開済みリリースを "下げる" には:

1. 該当 tag と Release を GitHub で **Pre-release に戻す** (削除ではなく)
2. `install.sh` は `VERSION` を指定しないと最新 Release を取りに行くので、上の操作で `latest` から外れる
3. 必要なら 1 つ前の tag を `latest` にマーク

破棄ではなく、次の PATCH を急いで出す方を優先すること。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| release workflow が失敗 | GitHub Actions ログ → 多くは musl ツールチェーン絡みの一時的失敗なので re-run |
| SHA-256 不一致 | 改竄疑い。Issue で報告 |
| tag を push したのに workflow が走らない | tag 名が `v*` パターンに合致しているか確認 (`v1.0`, `v1` は対象外) |
