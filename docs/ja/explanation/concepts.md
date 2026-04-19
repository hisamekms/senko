# 概念 — senko のドメインモデル

senko のコアには 5 つの集約 (aggregate) があります。これらの関係を押さえると、CLI・設定・API のどこに何が置かれているかが一気に理解しやすくなります。

```
  Project ─┬─ Task ─┬─ DoD items
           │        ├─ Dependencies (task ↔ task)
           │        ├─ Tags
           │        ├─ In-scope / Out-of-scope
           │        ├─ Metadata (JSON + validated by MetadataField)
           │        └─ (optional) Contract link
           │
           ├─ Contract ─┬─ DoD items
           │            ├─ Tags
           │            └─ Notes
           │
           ├─ MetadataField   (task.metadata の schema 定義)
           │
           └─ Members ─── User
                            └─ API keys (authentication)
```

## Task

senko のメインの作業単位。

**状態遷移 (forward only):**

```
draft → todo → in_progress → completed
                  ↓
               canceled   (どの状態からでも)
```

**主なフィールド:**

| フィールド | 型 | 説明 |
|---|---|---|
| `title` | string | タスク名 (必須) |
| `background` | string? | 背景・動機 |
| `description` | string? | 何をするか |
| `plan` | string? | どうやるか (Claude が埋めることが多い) |
| `priority` | P0〜P3 | 既定 P2、P0 が最優先 |
| `definition_of_done` | `{content, checked}[]` | 完了条件チェックリスト |
| `in_scope` / `out_of_scope` | string[] | スコープ明示 |
| `tags` | string[] | 分類 |
| `dependencies` | task id[] | `depends_on_task_id` が **全 completed** になるまで start 不可 |
| `metadata` | JSON | 任意 (下記 MetadataField で schema 化可能) |
| `branch` / `pr_url` | string? | git 連携 |
| `contract_id` | int? | Contract へのリンク (後述) |
| `assignee_user_id` | int? | 担当ユーザ |
| `task_number` | int | **プロジェクト内で一意** な連番。CLI が表示する番号 |

**ready の定義:** `status = todo` かつ依存タスクがすべて `completed`。`senko task next` はこの中から (priority → created_at → id) 順で 1 件を選びます。

## Contract

タスクより **粗い単位** で「何を達成したいか」を記述する集約。複数のタスクを束ねる "エピック" や "契約" のような役割です。

**主なフィールド:**

| フィールド | 型 | 説明 |
|---|---|---|
| `title` | string | Contract 名 |
| `description` | string? | 概要 |
| `definition_of_done` | `{content, checked}[]` | Contract レベルの完了条件 |
| `tags` | string[] | 分類 |
| `notes` | `{content, source_task_id, created_at}[]` | 作業中に得られた知見ログ |
| `metadata` | JSON | 任意 |

**タスクとの関係:** Task は `contract_id` で 1 つの Contract にリンクできます (optional)。`senko task list --contract <id>` でその Contract 配下の Task を列挙できます。

**完了判定:** Contract 自体には `status` がなく、**DoD 全チェック済み** を「is_completed」として扱います。

**Notes:** 作業中に判明した制約・背景をタスクから Contract に書き戻す運用を想定しています。`source_task_id` を持つので、「どのタスクで得られた知見か」を後から辿れます。

## Project

データの分離単位。全ての Task / Contract / MetadataField / Member は特定の Project に属します。

- ローカル利用では自動的に `default` プロジェクト (id=1) が作られる
- サーバ運用では `senko project create` で追加、メンバーを `project members add` で登録
- CLI はどの Project を操作するかを `--project <name>` フラグ / `SENKO_PROJECT` / `[project] name` の順で解決

## User / Members / API keys

- **User** はシステム全体で一意 (`username`, `sub`)
- **Member** は `(project, user, role)` の 3 つ組。role は `owner` / `member` / `viewer`
- **API key** は User に紐づき、複数発行可。`device_name` で個々を識別
- **Master key** (`[server.auth.api_key] master_key`) は User に紐づかず、`POST /users` 等のブートストラップ用

権限ロール:

| Role | できること |
|---|---|
| `owner` | Project 設定変更・メンバー管理・全ての Task/Contract 操作 |
| `member` | Task / Contract の CRUD |
| `viewer` | 読み取りのみ |

## Dependency

**Task → Task の有向辺**。"A depends on B" は "B が完了するまで A は start 不可" を意味します。

- 循環は CLI / API レベルで拒否 (loop 検出)
- `senko task deps add/remove/set/list` で編集
- `senko graph` (skill 経由) で依存関係をテキストグラフで可視化

Contract は依存関係を持ちません。Contract 間の順序付けは現状タグや命名規則で運用する想定です。

## MetadataField

Task / Contract の `metadata` (任意 JSON) に **型と必須性** を与えるための Project 単位の schema 定義。

```bash
senko project metadata-field add \
  --name estimate_points \
  --type number \
  --required-on-complete \
  --description "相対見積 (Fibonacci)"
```

- **`required_on_complete = true`** にすると、`task complete` 時にそのキーが存在しないとエラー
- `field_type` は `string` / `number` / `boolean`
- metadata 全体は `--metadata '{"estimate_points": 5}'` (shallow merge) / `--replace-metadata '...'` (全置換) で編集

workflow stage の `metadata_fields` と組み合わせると「plan stage で必ずこの値を埋める」といった運用が可能です ([explanation/workflow-stages.md](workflow-stages.md))。

## Hook

状態遷移の **前後** で任意のシェルコマンドを発火させる仕組み。詳細は [reference/hooks.md](../reference/hooks.md) と [explanation/runtimes.md](runtimes.md) を参照。

重要なのは「hook は常に runtime × action × name で識別される」という点:

```
<runtime>.<aggregate>_<action>.hooks.<name>
```

- runtime: `cli` / `server.remote` / `server.relay` / `workflow`
- action: `task_add` `task_ready` `task_start` `task_complete` `task_cancel` `task_select` / `contract_add` `contract_edit` `contract_delete` `contract_dod_check` `contract_dod_uncheck` `contract_note_add`

## Workflow stage

Claude Code skill が **読み取って段階ごとの instructions / hook を発火** するための「論理ステージ」。
組み込み stage: `task_add` `task_ready` `task_start` `task_complete` `task_cancel` `task_select` `branch_set` `branch_cleanup` `branch_merge` `pr_create` `pr_update` `plan` `implement` + `contract_*`。

これらは CLI の実コマンドに必ず 1:1 で対応するわけではなく、**エージェントが「今このフェーズに居る」と判断したタイミング** で読むドメイン定義です。詳細は [explanation/workflow-stages.md](workflow-stages.md)。

## なぜこうなっているか

- **Task と Contract を分けた理由**: Task は 1 context window で完結する粒度、Contract は複数タスクをまたぐ粒度。粒度が違うものを同じテーブルに混ぜると "完了" の意味がぶれる (Task は一度完了したら不変、Contract は DoD が段階的に埋まっていく)
- **MetadataField を Project 単位にした理由**: チームごとに「見積」「担当チーム」「リスクレベル」等の必須項目が違うため、固定カラムではなく schema 定義として外出しした
- **Runtime ごとに hook を分けた理由**: 同じプロジェクトを "手元の CLI" と "サーバ" 両方から触るとき、片方でしか走らせたくない hook (例: デスクトップ通知 vs. 監査ログ) が頻出するため

## 次に読むもの

- 使い分け判断 → [runtimes.md](runtimes.md)
- 内部の層構造 → [architecture.md](architecture.md)
- workflow stage の設計意図 → [workflow-stages.md](workflow-stages.md)
