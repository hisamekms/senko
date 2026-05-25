# `[workflow.*]` 設定

Claude Code skill が読む **論理ステージ定義**。どの runtime で動いていても読まれ得ます (実 runtime に対する状態遷移 hook とは別物)。

概念: [イベントドリブンなワークフロー](../../explanation/event-driven-workflow.md)

## `[workflow]` (トップレベル)

プロジェクト全体の workflow 既定値。

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `merge_via` | string | `"direct"` | `"direct"` (git merge) or `"pr"` (PR マージ検証が必要) |
| `auto_merge` | bool | `true` | `merge_via="direct"` 時、task complete で自動マージ |
| `branch_mode.type` | string | `"worktree"` | `"worktree"` (git worktree) or `"branch"` (通常 branch) |
| `branch_mode.create` | bool | `true` | `true`: skill が新規にリソースを作成 / `false`: 既存リソースを利用（skill は作成しない） |
| `merge_strategy` | string | `"rebase"` | `"rebase"` or `"squash"` |
| `branch_template` | string | `null` | ブランチ名テンプレート。`{{id}}` / `{{slug}}` が使える (例: `"senko/{{id}}-{{slug}}"`) |

### `branch_mode` の 4 組み合わせ

| `type` | `create` | 挙動 |
|---|---|---|
| `worktree` | `true`（既定） | skill がタスクごとに worktree を新規作成して switch する |
| `worktree` | `false` | 外部（人間 / 別ツール）が事前に作っておいた worktree を再利用する。worktree が見つからない場合は **fallback せずエラーで停止** |
| `branch` | `true` | skill が現在の checkout で branch を作成 / switch する（worktree なし） |
| `branch` | `false` | 現在のブランチで作業し、ブランチ操作は一切行わない |

**後方互換:** 旧文字列形式 `branch_mode = "worktree"` / `"branch"` は引き続き受理され、`{ type = <値>, create = true }` と等価扱いされます。既存の `config.toml` を書き換える必要はありません。新規に書く場合はテーブル形式を推奨します。

```toml
[workflow.branch_mode]
type = "worktree"
create = true
```

env override: `SENKO_MERGE_VIA` / `SENKO_AUTO_MERGE` / `SENKO_BRANCH_MODE_TYPE` / `SENKO_BRANCH_MODE_CREATE` / `SENKO_MERGE_STRATEGY` （旧 `SENKO_BRANCH_MODE` は撤去されました）

## `[workflow.<stage>]`

各 stage 共通のフィールド:

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `instructions` | string[] | `[]` | エージェントへの指示文 (stage 入場時に読ませる) |
| `hooks.<name>` | HookDef | `{}` | この stage で発火する hook (`prompt` フィールドが使える) |
| `metadata_fields` | object[] | `[]` | この stage で入力させる metadata。値は task/contract の metadata に shallow merge される |

stage 固有の追加キー:

| Stage | キー | 型 | 説明 |
|---|---|---|---|
| `workflow.task_add` | `default_dod` | string[] | 新規タスクのデフォルト DoD |
| `workflow.task_add` | `default_tags` | string[] | デフォルトタグ |
| `workflow.task_add` | `default_priority` | string | デフォルト priority |
| `workflow.plan` | `required_sections` | string[] | 計画ドキュメントに必須のセクション |

**未知のキーは破棄されず保持** され、`senko config --output json` で参照できます。独自 skill が独自キーを読む運用が可能。

## 組み込み stage

skill が発火させる stage:

```
task_add       task_publish   task_start    task_complete
task_cancel    task_select    branch_set    branch_cleanup
branch_merge   pr_create      pr_update     plan
implement      contract_add   contract_edit contract_delete
contract_dod_check   contract_dod_uncheck   contract_note_add
```

> 現行の bundled workflow で実際に発火するのは `task_*` + `plan` / `implement` / `branch_*` / `pr_*` + `contract_add` / `contract_note_add` / `contract_dod_check`。`contract_edit` / `contract_delete` / `contract_dod_uncheck` は予約済みだが skill 標準シナリオでは未発火。

**任意の名前を受け付ける** ので、プロジェクト独自の stage も定義可能 (例: `security_review`)。

## Hook の `prompt` フィールド

workflow hook は **shell コマンド発火** に加えて **エージェント指示の注入** という特殊挙動を持ちます:

```toml
[workflow.contract_note_add.hooks.review]
command = "true"                                     # shell 側は no-op
prompt = "既存のノートに同じ観察が無いか確認してから追加"
when = "pre"
```

- `command` は shell で実行される (通常の hook と同じ)
- `prompt` は skill がエージェントへの instruction として構築する
- `command` 側で何かする必要が無ければ `command = "true"` にする

## `metadata_fields`

```toml
[[workflow.task_add.metadata_fields]]
key = "team"
source = "value"
value = "backend"

[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "フィボナッチで見積もってください"
```

| キー | 型 | 説明 |
|---|---|---|
| `key` | string | metadata のキー |
| `source` | `"value"` / `"prompt"` | 値の出所 |
| `value` | string? | `source="value"` の場合に注入する固定値 |
| `prompt` | string? | `source="prompt"` の場合にエージェントが使う問いかけ |

Project の `metadata_fields` で `required_on_complete = true` の field を workflow で毎回注入するパターンが典型。

## 最小例

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"
merge_via = "pr"

[workflow.task_add]
default_dod = ["Unit tests pass", "Docs updated"]
default_priority = "p2"

[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria"]
instructions = ["plan は task.plan フィールドに保存する"]

[workflow.task_complete.hooks.ci_check]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"
mode = "sync"
on_failure = "abort"
```

実例集: [Workflow stage の実例](../../guides/cli/workflow-stages.md)
