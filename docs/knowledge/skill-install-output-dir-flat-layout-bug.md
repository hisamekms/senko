---
id: skill-install-output-dir-flat-layout-bug
title: skill-install --output-dir はサブディレクトリ構造も維持する必要がある
description: SKILL.md 自身が ${CLAUDE_SKILL_DIR}/workflows/xxx.md のような相対パスを参照するため、--output-dir でのインストールもディレクトリ構造を保持しないと参照が壊れる
tags:
  - senko-cli
  - skill-install
  - claude-code-skill
created_at: 2026-07-20
updated_at: 2026-07-20
---

## 概要

`senko skill-install --output-dir <dir>` は、v0.47.0 時点で全インストール対象ファイルを `dir.join(filename)`（`segments.last()` のみ）でフラットに書き込んでいた。一方 `.claude/skills/senko/SKILL.md` の中身は変更されず、`${CLAUDE_SKILL_DIR}/workflows/auto-select.md` のようにサブディレクトリ付きの相対パスで各ワークフローファイルやスクリプトを参照する。そのため `--output-dir` でインストールすると、SKILL.md が参照するパスと実際のファイル配置が食い違い、`/senko` 実行時にワークフロー手順書やスクリプトが読めなくなるバグがあった。

## 詳細

- インストール対象ファイルは `build.rs` の `scan_skill_dir` / `scan_agents_dir` で走査され、`segments`（例: `["skills", "senko", "workflows", "auto-select.md"]`）付きで `INSTALLABLE_FILES` に埋め込まれる。
- デフォルトの `.claude/` 配下へのインストール（`output_dir` 未指定）では `segments` をそのまま `fold` してパスを組み立てるため、ディレクトリ構造が正しく再現される。
- `--output-dir` 指定時だけ `segments.last()`（ファイル名のみ）を使っていたため、`workflows/`・`scripts/` などのサブディレクトリが失われ、SKILL.md の参照と実体が不整合になっていた。
- `src/presentation/cli/skill.rs` に `output_dir_segments()` を追加し、`["skills", "senko"]` または `["agents"]` の先頭プレフィックスを取り除いた残りのセグメントを使ってパスを組み立てるよう修正。これにより `--output-dir` でも `workflows/xxx.md` のような相対構造が維持される。

## 解決策 / 推奨事項

- `senko skill-install` の出力先を変える機能を弄るときは、`INSTALLABLE_FILES` の `segments` を「ファイル名だけ」に潰さず、スキル/エージェントルートからの相対パスとして扱うこと。
- SKILL.md や他の埋め込みファイルが `${CLAUDE_SKILL_DIR}/...` のような相対パス参照を含む場合、インストーラのどのモードでもその参照解決に必要なディレクトリ構造を維持しているか確認する。
- 関連テスト: `src/presentation/cli/skill.rs` の `skill_install_with_output_dir_creates_files`。

## 参考

- `src/presentation/cli/skill.rs`
- `.claude/skills/senko/SKILL.md`（`${CLAUDE_SKILL_DIR}/workflows/...` の参照元）
- `docs/en/guides/cli/skill-install.md` / `docs/ja/guides/cli/skill-install.md`
