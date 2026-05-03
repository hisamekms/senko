# Proposal: `senko task list` / `senko contract list` への `--order-by` / `--order` フラグ追加

Status: **Deferred**（設計のみ、実装は未着手）
Date: 2026-05-03
Related: Task #430（HTTP API への `order_by` / `order` クエリパラメータ追加と複合カーソル実装）

## 概要

Task #430 で HTTP API (`GET /api/v1/projects/{id}/tasks`, `.../contracts`) に `order_by` / `order` クエリパラメータと複合カーソルを実装した。同等の機能を CLI (`senko task list`, `senko contract list`) でも露出させる提案。

## 動機

- HTTP API と CLI が共通の `ListTasksFilter` / `ListContractsFilter` を組み立てる構造になっており、片方だけソート指定可能なのは対称性に欠ける
- シェルから「最近更新されたタスクのトップ10」のような頻出クエリを簡潔に書けると運用が楽になる（`senko task list --order-by updated_at --order desc --limit 10`）
- e2e shell テストでも CLI 経由のソート挙動を直接検証できる

## なぜ Task #430 のスコープに含めなかったか

Task #430 の主目的（ダッシュボードの再ページング解消）は HTTP API だけで達成可能。CLI フラグ追加は対称性のための拡張であり、独立に実施可能なため別タスク化した。

## 提案する CLI シンタックス

```bash
senko task list --order-by <id|updated_at|priority> --order <asc|desc> [--limit N] [--after <cursor>]
senko contract list --order-by <id|updated_at> --order <asc|desc> [--limit N] [--after <cursor>]
```

### 例

```bash
# 直近更新順 上位10件
senko --output text task list --order-by updated_at --order desc --limit 10

# Ready tasks を優先度順（P0 先頭）
senko --output text task list --ready --order-by priority --limit 20

# Contracts 直近更新順
senko --output text contract list --order-by updated_at --order desc --limit 20

# ページネーション
PAGE1=$(senko task list --order-by updated_at --order desc --limit 50)
CURSOR=$(echo "$PAGE1" | jq -r '.next_cursor')
[ "$CURSOR" != "null" ] && \
  senko task list --order-by updated_at --order desc --limit 50 --after "$CURSOR"
```

## 実装範囲（推定）

| 場所 | 変更内容 |
|---|---|
| `src/presentation/cli/mod.rs` | `task list` / `contract list` サブコマンドに `--order-by` と `--order` フラグ追加（clap `value_parser` で enum バリデーション） |
| `src/presentation/cli/handlers.rs` | フラグ値を `TaskOrderBy` / `ContractOrderBy` / `ListOrder` に変換して filter に渡す |
| `src/presentation/cli/handlers.rs`（text 出力） | 既存の `... more: --after <cursor>` 出力を `--order-by` 指定時にも適切に表示 |
| `tests/e2e/test_cli_*.sh` | CLI 経由のソート挙動テスト（最小1〜2ケース） |
| `docs/book/` | コマンドリファレンスに新フラグを記載 |

推定変更量: +50〜100行。Task #430 の domain / application 層の変更がそのまま再利用できるため、コストは低い。

## 設計上の注意点

- `--order-by`/`--order` の clap value_parser は HTTP API のバリデーションと **完全に同じ受理セット**にすること（不一致だと CLI ↔ API で挙動が割れる）
- CLI は HTTP API を経由せず application service を直接呼ぶため、Cursor の base64 エンコード/デコードはドメイン層の `Cursor::encode_*` / `Cursor::decode` を直接使う（既に Task #430 で公開 API になっている前提）
- CLI ヘルプ文字列で「order_by != id のとき after は同じ order_by 指定が必須」と明記する（不整合は ApiError ではなく DomainError として現れる）

## オープンな問い

- `--order-by` のショートエイリアス（例: `-O updated_at`）は提供するか？ 既存の他フラグ（`--limit`, `--after`）にショート形がないため統一性のため不要と判断
- `senko task next` の優先度ロジックと `--order-by priority` の関係を整理する必要があるか？ → `next_task` は専用の選択ロジック（assignee / dependency / 優先度の複合）を持つので、`list --order-by priority` とは別物として扱う
