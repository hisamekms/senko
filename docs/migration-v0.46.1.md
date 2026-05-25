# senko config.toml マイグレーションガイド (v0.46.0 → v0.46.1)

このドキュメントは、senko v0.46.0 以前で運用していた `branch_mode` 設定と `SENKO_BRANCH_MODE` 環境変数を、v0.46.1 で導入されたテーブル型スキーマに移行するための手順書です。

## 概要

v0.46.1 で `workflow.branch_mode` が **文字列 enum**（`"worktree"` / `"branch"`）から **`type` + `create` の 2 フィールドを持つテーブル型**へ拡張されました。これにより「worktree か branch か」と「skill が新規リソースを作成するか / 既存リソースを再利用するか」を直交に制御できます。

このマイグレーションは **非破壊的** です。

- 旧文字列形式 `branch_mode = "worktree"` / `"branch"` は引き続き受理され、`{ type = <値>, create = true }` と等価扱いされます。既存の `config.toml` を書き換える必要はありません。
- ただし **環境変数は撤去**されました（`SENKO_BRANCH_MODE` → `SENKO_BRANCH_MODE_TYPE` / `SENKO_BRANCH_MODE_CREATE`）。CI / デプロイ環境で旧変数を使っている場合は新変数へ置換が必要です。

## AI への指示

ユーザーの既存 `config.toml` および環境変数定義（`.envrc` / CI 設定 / Dockerfile / Kubernetes manifest 等）を読み取り、以下のルールに該当する箇所のみ変換してください。存在しない箇所は無視してください。

---

## ルール 1: `branch_mode` 文字列 → `[workflow.branch_mode]` テーブル

`[workflow]` セクション直下の `branch_mode = "..."` 行を、独立した `[workflow.branch_mode]` テーブルに展開する。

**Before:**
```toml
[workflow]
branch_template = "feat/{{id}}-{{slug}}"
branch_mode = "worktree"
```

**After:**
```toml
[workflow]
branch_template = "feat/{{id}}-{{slug}}"

[workflow.branch_mode]
type = "worktree"
create = true
```

**変換ルール:**

- `branch_mode = "worktree"` → `[workflow.branch_mode]` テーブルに `type = "worktree"` と `create = true` を書く
- `branch_mode = "branch"` → 同様に `type = "branch"` と `create = true`
- `create` を省略した場合のデフォルトは `true`
- 旧形式のまま放置しても動作します。書き換えは任意です（推奨はテーブル形式）

---

## 新形式の 4 組み合わせ

`type` と `create` の組み合わせで以下 4 通りの挙動を取れます。

| `type` | `create` | 挙動 |
|---|---|---|
| `worktree` | `true`（既定） | skill がタスクごとに worktree を新規作成して switch する |
| `worktree` | `false` | 外部（人間 / 別ツール）が事前に作っておいた worktree を再利用する。worktree が見つからない場合は **fallback せずエラーで停止** する |
| `branch` | `true` | skill が現在の checkout で branch を作成 / switch する（worktree なし） |
| `branch` | `false` | 現在のブランチで作業し、ブランチ操作は一切行わない |

**重要:** `type = "worktree", create = false` のときに worktree が存在しないと、skill は **fallback せずにエラーで停止** します（カレントディレクトリへの暗黙的な退避は行われません）。外部で worktree を準備しておく前提のセットアップでのみ使用してください。

---

## ルール 2: 環境変数の置換

`SENKO_BRANCH_MODE` は v0.46.1 で撤去されました。代わりに `SENKO_BRANCH_MODE_TYPE` と `SENKO_BRANCH_MODE_CREATE` を使用します。

### リネームされた環境変数

| v0.46.0 以前 | v0.46.1 以降 | 値 |
|---|---|---|
| `SENKO_BRANCH_MODE=worktree` | `SENKO_BRANCH_MODE_TYPE=worktree` | `worktree` / `branch` |
| `SENKO_BRANCH_MODE=branch` | `SENKO_BRANCH_MODE_TYPE=branch` | 同上 |
| (該当なし) | `SENKO_BRANCH_MODE_CREATE=true` | `true` / `false` / `1` / `0` / `yes` / `no` |

### 撤去された環境変数

| 変数 | 備考 |
|---|---|
| `SENKO_BRANCH_MODE` | `SENKO_BRANCH_MODE_TYPE` に置換してください。設定したままにしても v0.46.1 では効果がありません |

**Before（`.envrc` 等）:**
```sh
export SENKO_BRANCH_MODE=branch
```

**After:**
```sh
export SENKO_BRANCH_MODE_TYPE=branch
export SENKO_BRANCH_MODE_CREATE=false   # 必要に応じて。デフォルトは true（= worktree/branch を skill が作成）
```

---

## 推奨マイグレーション手順

1. **環境変数の置換（必須）**
   - `.envrc` / `.env` / CI 設定 / Dockerfile / Kubernetes manifest を `grep -rn 'SENKO_BRANCH_MODE\b'` で洗い出し、`SENKO_BRANCH_MODE_TYPE` / `SENKO_BRANCH_MODE_CREATE` に置換する
   - 旧変数は撤去されているため、設定しても効果がない
2. **`config.toml` の書き換え（任意）**
   - 既存の `branch_mode = "..."` 行はそのままでも動作する
   - 新規プロジェクトや書き換えを行うプロジェクトでは `[workflow.branch_mode]` テーブル形式を推奨
3. **挙動の選択**
   - 既存の worktree 運用を続けたい場合: `{ type = "worktree", create = true }`（= 既定値）のままで OK
   - 手動 / 別ツールで worktree を作っている場合: `{ type = "worktree", create = false }` に切り替えると skill は worktree を作らず既存のものを再利用する
   - worktree を使わず通常 branch で運用したい場合: `{ type = "branch", create = true }` で skill が branch を作成 / switch する
   - 既に CI 等で branch を準備してから skill を起動する場合: `{ type = "branch", create = false }` で skill はブランチ操作を一切行わない

---

## 変更なし（そのまま使える）

- `[workflow]` の `merge_via`, `auto_merge`, `merge_strategy`, `branch_template`
- 環境変数 `SENKO_MERGE_VIA` / `SENKO_AUTO_MERGE` / `SENKO_MERGE_STRATEGY`
- `branch_mode = "worktree"` / `"branch"` の旧文字列形式（後方互換）

---

## 関連ドキュメント

- リファレンス: [`[workflow.*]` 設定](ja/reference/config/workflow.md) （`branch_mode` 節を参照）
- 設定例: [`Workflow stage の実例`](ja/guides/cli/workflow-stages.md)
- 入門: [ローカル SQLite](ja/getting-started/local-sqlite.md)
