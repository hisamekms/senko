# イベントドリブンなワークフロー

> 3 つの柱のうち **柱 1**。[コアコンセプト: 3 つの柱](core-concept.md) で全体像を先に確認してから読んでください。

## 解きたい問題

プロジェクトには、必ず **プロジェクト固有のルール** があります:

- このリポジトリは PR マージ後に初めてタスクを完了にしてよい
- ブランチ名は `<prefix>/<task-id>-<slug>` で統一
- 見積 (estimate_points) が無いタスクは完了できない
- タスク完了時に SIEM に監査ログを送る
- plan フェーズでは必ず「受け入れ基準」セクションを書く

これらをエージェントに毎回プロンプトで教え直すのは非現実的で、ただ巨大化させるだけです。senko は **ドメインイベントを起点にルールを自動注入・検証する** アプローチを取ります。

## アーキテクチャの全体像

```
                 ┌─ CLI subcommand ─┐
  user / agent ──┤                  ├── 状態遷移 ──┐
                 └─ REST API ───────┘              │
                                                   ▼
                                       ┌─ HookTrigger ─┐
                                       │   task_add    │
                                       │   task_start  │
                                       │   task_complete
                                       │   contract_*  │
                                       └───┬───────────┘
                                           │
              ┌────── hooks (runtime × action) ──────┐
              │  [cli.task_complete.hooks.ci_green]   │── 柱 1: プロジェクト固有ルール
              │  [server.remote.task_add.hooks.audit] │   の自動発火
              │  [server.relay.task_start.hooks.log]  │
              └────────────────────────────────────────┘

              ┌────── workflow stages (論理フェーズ) ────┐
              │  [workflow.plan]                          │── 柱 1: エージェントの
              │    instructions = [...]                   │   論理フェーズへの指示
              │    hooks.<name> (prompt 付き)             │
              │  [workflow.branch_set]                    │
              │  [workflow.task_complete] ...             │
              └────────────────────────────────────────────┘
```

senko は **2 系統のメカニズム** を同じイベント源から分岐させます:

1. **hook** — senko binary が状態遷移の前後でシェルコマンドを発火
2. **workflow stage** — Claude Code skill が「今自分はこのフェーズにいる」と判断したタイミングで読む指示セット

## メカニズム 1: Hook

### 発火ポイント

hook は **aggregate × action** で識別される `HookTrigger` に紐づきます:

| Aggregate | Action | 発火タイミング |
|---|---|---|
| Task | `task_add` | 作成前後 |
| Task | `task_publish` | draft → todo |
| Task | `task_start` | todo → in_progress (`task next` での自動選択含む) |
| Task | `task_complete` | in_progress → completed (DoD 検証後) |
| Task | `task_cancel` | canceled 遷移 |
| Task | `task_select` | `task next` で候補を決定した時点 (選べたか否かは `on_result` で分岐) |
| Contract | `contract_add` / `contract_edit` / `contract_delete` | CRUD |
| Contract | `contract_dod_check` / `contract_dod_uncheck` | DoD 更新 |
| Contract | `contract_note_add` | Notes 追加 |

→ envelope / フィールド詳細: [Hooks リファレンス](../reference/hooks.md)

### Runtime スコープ

同じ `task_complete` でも、**どの runtime で動作中か** によって発火する section が変わります:

```
cli で実行 → [cli.task_complete.hooks.*] だけ発火
server.remote で実行 → [server.remote.task_complete.hooks.*] だけ発火
server.relay 経由 → [server.relay.task_complete.hooks.*] だけ発火
```

→ 使い分けの指針: [Runtime の使い分け](runtimes.md)

### Hook の振る舞いを決める 4 つのキー

```toml
[cli.task_complete.hooks.ci_green]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"          # 状態遷移の前 or 後 (post 既定)
mode = "sync"         # 完了を待つ or fire-and-forget
on_failure = "abort"  # 非ゼロ終了時: abort / warn / ignore
```

- **`when = "pre"` + `mode = "sync"` + `on_failure = "abort"`** の組合せだけが **状態遷移をキャンセルできる** (他の組合せでは warn に降格)
- async は fire-and-forget なので abort 不可
- skill から見える "ブロッキング検証" は常にこの 3 点セットになる

### プロジェクト固有ルールの自動注入パターン

| やりたいこと | 置き方 |
|---|---|
| タスク完了前に CI 通過を必須に | `[cli.task_complete.hooks.ci] when = pre, mode = sync, on_failure = abort` |
| タスク追加時に監査ログを送る (サーバ側) | `[server.remote.task_add.hooks.audit] when = post, mode = async` |
| `task next` が空振りした時に通知 | `[cli.task_select.hooks.idle] on_result = "none"` |
| Contract に Note を書いた時に Slack に貼る | `[server.remote.contract_note_add.hooks.slack]` |

## メカニズム 2: Workflow stage

hook はコマンドを発火しますが、**エージェントに「何を考えて行動するか」を教える** には別のレイヤが要ります。それが **workflow stage** です。

### Stage とは何か

Claude Code skill は「今 plan している」「今 implement している」のような **論理的フェーズ** を持ちます。これらは必ずしも CLI コマンドと 1:1 対応しません。

- `plan` フェーズ: まだ `senko task edit --plan ...` は叩かれていないが、エージェントは設計を練っている
- `implement` フェーズ: `senko task start` は済んでおり、コードを書いている最中
- `branch_set` フェーズ: git branch を切る直前。命名テンプレや pre-check を差し込みたい

これを「CLI の action」と分けた **論理ステージ** として `[workflow.<stage>]` 配下に置きます。

### 組み込み stage

| Stage | 意味 |
|---|---|
| `task_add` | 新しいタスクを追加する前後 |
| `task_publish` | draft → todo 遷移 |
| `task_start` | todo → in_progress (または `task next` での自動選択) |
| `task_complete` | in_progress → completed |
| `task_cancel` | canceled 遷移 |
| `task_select` | `task next` でタスクを選ぼうとする時点 |
| `branch_set` | 作業ブランチを切る直前 |
| `branch_cleanup` | ブランチを消す前 |
| `branch_merge` | マージ操作の直前 |
| `pr_create` | PR 作成前 |
| `pr_update` | PR 更新前 |
| `plan` | 設計を文章化するフェーズ |
| `implement` | 実装フェーズ |
| `contract_add` / `contract_edit` / `contract_delete` | Contract の CRUD |
| `contract_dod_check` / `contract_dod_uncheck` | Contract DoD の更新 |
| `contract_note_add` | Contract にノートを追記する前 |

**任意の名前を受け付ける**ので、独自 skill から参照する用途なら `[workflow.my_phase]` のようにプロジェクト独自 stage を追加しても構いません。組み込み以外は senko 本体が発火させませんが、`senko config --output json` で素通しで公開されます。

### Stage が持てるフィールド

各 stage は `[workflow.<stage>]` 配下で以下を宣言できます:

| キー | 型 | 役割 |
|---|---|---|
| `instructions` | string[] | エージェントにこの stage で守らせたい指示文 |
| `hooks.<name>` | HookDef | シェル hook の発火 (他 runtime の hook と同じスキーマ) |
| `metadata_fields` | object[] | この stage で入力・注入させる metadata key と値 |

stage 固有の追加キー (例):

| Stage | キー | 役割 |
|---|---|---|
| `task_add` | `default_dod` / `default_tags` / `default_priority` | 新規タスクのデフォルト値 |
| `plan` | `required_sections` | 計画ドキュメントに必須のセクション名 |

未知のキーは **破棄されず保持** され、外部スクリプトが `senko config --output json` 経由で参照可能です。

### Stage hook と runtime hook の違い

| Hook の場所 | 発火主体 | タイミング |
|---|---|---|
| `[cli/server.*/server.relay.<action>.hooks.<name>]` | senko binary | 状態遷移の前後 (自動) |
| `[workflow.<stage>.hooks.<name>]` | Claude Code skill | skill が stage に入ったと判断した時 |

workflow hook 特有のフィールドとして **`prompt`** があります。skill はこの文字列を **エージェント自身への instruction** として読み込みます (shell コマンドではなくプロンプト拡張)。

```toml
[workflow.contract_note_add.hooks.review_before_note]
command = "true"                                       # no-op
prompt = "Skip the note if the same observation already exists in earlier notes."
when = "pre"
```

この例では、Contract にノートを追加する直前に「同じ観察が既存ノートに無いか確認しろ」とエージェントに指示が注入されます。

## `metadata_fields` によるスキーマ注入

stage で必ず埋めさせたい metadata を宣言できます:

```toml
[[workflow.task_add.metadata_fields]]
key = "team"
source = "value"
value = "backend"

[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "フィボナッチ数列で見積もってください"
```

`source` は:

- `value`: 固定値を注入
- `prompt`: `prompt` の文言でエージェントに入力を求める
- `env`: 環境変数から取得
- `command`: シェルコマンドの出力を使う

### <a id="metadata-field"></a>MetadataField (Project 単位の schema)

stage 側の `metadata_fields` は **その stage で埋める値** を規定しますが、プロジェクト全体として **`metadata` に何が許可・必須か** を定義するのが **MetadataField** です。

```bash
senko project metadata-field add \
  --name estimate_points \
  --type number \
  --required-on-complete \
  --description "相対見積 (Fibonacci)"
```

- `field_type` は `string` / `number` / `boolean`
- **`required_on_complete = true`** にすると、`task complete` 時にそのキーが無ければエラー
- metadata 全体は `--metadata '{"estimate_points": 5}'` (shallow merge) / `--replace-metadata '...'` (全置換) で編集

stage の `metadata_fields` (注入側) と Project の MetadataField (検証側) を組み合わせると、「plan stage で必ず埋める → complete 時に検証」という連携が作れます。

## 典型パターン

### 1. plan stage で設計フォーマットを強制

```toml
[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria", "Risks"]
instructions = [
  "plan は task.plan フィールドに保存する",
  "実装着手前に必ず human にレビューを依頼する",
]
```

### 2. branch_set で命名規則を統一

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"

[workflow.branch_set]
instructions = ["feature/ / fix/ / chore/ prefix は不可 (branch_template で統一済)"]
```

### 3. task_complete で CI 通過を必須に

```toml
[cli.task_complete.hooks.ci_green]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"
mode = "sync"
on_failure = "abort"
```

### 4. task_select が空振りした時に通知

```toml
[cli.task_select.hooks.idle_notify]
command = "notify-send 'No ready tasks. Run /senko task list to review.'"
on_result = "none"
```

## skill との連動

`senko skill-install` で配置される SKILL.md は、内部で `senko config --output json` を叩いて現在の workflow 設定を読み、stage ごとの `instructions` / hook の `prompt` を **その時点のエージェント指示** として組み立てます。

つまり運用フロー:

1. プロジェクトごとに `.senko/config.toml` に `[workflow.*]` / `[cli.*]` / `[server.*.*]` を書く
2. 開発者が `senko skill-install` で SKILL.md を更新
3. Claude Code が `/senko` 実行時に workflow 設定を参照しながら動く

## 設計判断

- **なぜ hook と workflow stage を両方用意したか**: hook はシェルコマンドを発火 (機械的検証) 、workflow stage はエージェントに指示 (判断を伴う検証)。両方同じイベントに紐付く別ベクトル
- **なぜ runtime ごとに hook を分けたか**: 同じイベントでも CLI (開発者のデスクトップ通知) とサーバ (SIEM 送信) は分けたい
- **なぜ `pre + sync + abort` だけ状態遷移をキャンセルできるか**: async は完了を待てない以上、遷移前に "止める" 意思決定ができないため

## 次に読むもの

- 柱 2 → [「次の 1 件」に集中できる実行モデル](task-decomposition.md)
- 柱 3 → [Contract による全体像の保持](contract.md)
- Runtime の使い分け → [Runtime の使い分け](runtimes.md)
- Hook の envelope / 発火タイミング一覧 → [Hooks リファレンス](../reference/hooks.md)
- `[workflow.*]` の TOML 詳細 → [`[workflow.*]` 設定](../reference/config/workflow.md)
- 実例集 → [Workflow stage の実例](../guides/cli/workflow-stages.md) / [`[cli.*]` hook の実例](../guides/cli/hooks.md)
