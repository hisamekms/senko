---
id: e2e-macos-bsd-portability
title: e2e/リリーススクリプトは macOS の BSD ツールで壊れる書き方を避ける
description: GNU coreutils 前提のシェルスクリプトが macOS（BSD ツール + bash 3.2）で失敗するパターン集。v0.50.0 リリース時に e2e が5ファイル失敗して発覚した。
tags:
  - e2e
  - shell
  - macos
  - bsd
  - portability
created_at: 2026-07-25
updated_at: 2026-07-25
---

## 概要

e2e テストとリリーススクリプトは Linux（devcontainer）と macOS の両方で実行される。GNU coreutils / bash 4+ 前提の書き方をすると macOS（BSD ツール + 標準 bash 3.2）で失敗する。v0.50.0 リリース時の e2e 実行で 5 ファイル + release-boot-check.sh が一斉に失敗して発覚した。

## 詳細（失敗したパターンと修正）

| GNU 前提の書き方 | macOS での症状 | portable な書き方 |
|---|---|---|
| `grep -P '#\K[0-9]+'` | `invalid option -- P`（BSD grep に PCRE なし） | `grep -o '#[0-9][0-9]*' \| head -n1 \| tr -d '#'` |
| `paste -sd ','`（stdin 暗黙） | `usage: paste ...`（file 引数必須） | `paste -sd ',' -` と `-` を明示 |
| `"${arr[@]}"`（空配列 + `set -u`） | bash 3.2 で `arr[@]: unbound variable` | `${arr[@]+"${arr[@]}"}` |
| `touch -d '14 days ago' f` | BSD touch は相対 `-d` 非対応 | `perl -e 'my $t = time - $ARGV[0]; utime($t,$t,$ARGV[1])' <sec> f` |
| `tr '-_' '+/'` | 先頭 `-` をオプション解釈（`illegal option -- _`） | 文字順を入れ替え `tr '_-' '/+'` |
| `timeout 3s cmd` | `timeout: command not found`（coreutils 未導入） | バックグラウンド起動 + `sleep 3; kill $PID; wait` |
| `base64 -d`（unpadded base64url） | パディング欠落で末尾ブロックが黙って欠ける | `tr '_-' '/+'` で標準文字に戻し、長さ %4 に応じて `=` を補ってから decode |

- senko の API cursor（`next_cursor`）は **unpadded base64url**。テストで decode するときは必ずパディング復元が要る（test_serve_api.sh 参照）。
- `stat` はすでに `stat -c %Y || stat -f %m` の両対応イディオムが `.claude/skills/senko/scripts/senko-narrative.sh` にある。

## 解決策 / 推奨事項

- 新しい e2e スクリプトを書くときは上記の portable 側の書き方を使う（既存の `tests/e2e/*.sh` に実例あり）。
- リリースは macOS ホストからも実行されるため、`scripts/release-*.sh` も GNU 専用コマンド（`timeout` 等）を直接使わない。
- e2e を実行するとき `mise run e2e 2>&1 | tail` のようにパイプすると exit code がパイプ先のものになる点にも注意（失敗を見逃す）。

## 参考

- 修正コミット: `fix(e2e): make test scripts portable to macOS/BSD tools`（v0.50.0 に同梱）
- 修正コミット: `fix(scripts): emulate GNU timeout in release-boot-check for macOS`
