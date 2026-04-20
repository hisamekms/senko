# コアコンセプト: 3 つの柱

senko は「タスク管理ツール」というよりも **AI エージェントが自律的に作業を進めるためのワークフローオーケストレータ** です。

一般的なタスク管理ツールは人間が人間に向けて使うことを前提としていますが、senko は **「プロジェクト固有の進め方を codify してエージェントに教える」** ことに比重を置いています。Claude Code との連携を主眼に、以下 3 つの柱で AI エージェントの自律動作を支えます。

## 柱 1: イベントドリブンなワークフロー

プロジェクトには必ず「このリポジトリではタスク完了前に CI をグリーンにする」「ブランチ名はこのテンプレ」「DoD に監査観点を必ず含める」といった **独自ルール** があります。これを人間が毎回エージェントに教え直すのは現実的でなく、逆にプロンプトに全て書き込むと肥大化してスキルやモデルのコンテキストを食い潰します。

senko は **状態遷移イベント (task_add / task_start / task_complete / contract_note_add など)** を起点に、ルールをエージェントの行動に **自動で注入・検証** する仕組みを備えます。

- **hook**: 状態遷移の前後にシェルコマンドを発火 (CI チェック、通知、監査ログ送信)
- **workflow stage**: Claude Code skill がフェーズごとに読む「指示とチェックリスト」の器 (plan・implement・branch_set 等)
- **runtime 分離**: 同じイベントでも cli / server.remote / server.relay のどこで発火するかを設定単位で切り替えられる

→ 深掘り: [イベントドリブンなワークフロー](event-driven-workflow.md)

## 柱 2: 「次の 1 件」に集中できる実行モデル

AI エージェントに「このプロジェクトを全部進めて」と依頼するのは典型的なアンチパターンです。巨大プロンプトに context を詰め込めば、判断はぶれ、コンテキストは汚染され、1 回の session で完結できない。

senko は **作業を依存関係と優先度を持つタスク列に分割** し、エージェントに「今はこの 1 件だけをやる」と明示させる実行モデルを提供します。

- **Task = 1 context window で完結する粒度** — 1 タスク完了ごとに session を閉じて、次のタスクでは context をリセットできる
- **依存が解けた ready タスクから自動選択** — エージェントは次に何をやるか考えない。`senko task next` が priority → created_at → id で決める
- **並列 pick** — ready なタスクが複数あれば、別々の session / worktree から同時に取りに行ける

→ 深掘り: [「次の 1 件」に集中できる実行モデル](task-decomposition.md)

## 柱 3: Contract で全体像を保持

Task は context window 内で完結する粒度なので、1 タスクで得た知見は単体では失われがちです。一方、機能追加や移行のように **複数タスクをまたぐ作業** には「全体像」「累積した制約」「決定の履歴」を保持する器が必要です。

senko では **Contract** がその役割を担います。

- Contract は複数の Task を束ね、**DoD と Notes** を横断的に保持する
- Task 完了時に `source_task_id` 付きで Notes を書き戻すと、どのタスクで得られた知見かを後から辿れる
- Contract は「エピック」「設計判断の集合」「移行の目的」に近い粒度

→ 深掘り: [Contract による全体像の保持](contract.md)

## 3 つの柱がどう組み合わさるか

典型的な流れ:

```
  Contract 作成            ← 柱 3: 何を達成したいか宣言
     │
     ▼
  Task 分解                 ← 柱 2: 依存と優先度を付けて列にする
     │
     ▼
  task_add hook / stage    ← 柱 1: DoD 雛形 / 命名規則 / 必須 metadata を自動注入
     │
     ▼
  task next で 1 件選択     ← 柱 2: エージェントは「今の 1 件」だけ
     │
     ▼
  plan / implement stage   ← 柱 1: 各フェーズの instructions と検証 hook
     │
     ▼
  task complete hook       ← 柱 1: CI / DoD / PR マージ検証
     │
     ▼
  Contract に Note 追記    ← 柱 3: 得られた知見を全体像へ還元
     │
     ▼
  次のタスクが ready に     ← 柱 2: 依存解除で自動で次が浮上
```

## 支える補助概念

3 つの柱を下支えする概念も押さえておきましょう。

| 概念 | 役割 | 詳細 |
|---|---|---|
| **Project** | データの分離単位。全 Task / Contract / Member が属する | [データモデル](../reference/data-model.md) |
| **User / Member / API key** | 誰が何の role で何のプロジェクトを操作できるか | [データモデル](../reference/data-model.md) |
| **MetadataField** | `task.metadata` / `contract.metadata` の型付き schema (プロジェクト単位) | [イベントドリブンなワークフロー](event-driven-workflow.md#metadata-field) |
| **Runtime** | 実行モード (cli / server.remote / server.relay / workflow)。設定と hook の有効範囲を決める | [Runtime の使い分け](runtimes.md) |
| **Dependency** | Task → Task の有向辺。"B 完了まで A は start 不可" | [「次の 1 件」に集中できる実行モデル](task-decomposition.md#dependency) |

## 設計判断の背景

- **なぜ Task と Contract を分けたか**: 粒度が違うものを同じテーブルに混ぜると "完了" の意味がぶれます。Task は一度完了したら状態不変。Contract は DoD が段階的に埋まっていく進行形の器
- **なぜ MetadataField を Project 単位の schema にしたか**: チームごとに「見積」「担当チーム」「リスクレベル」等の必須項目が違うため、固定カラムではなく schema 定義として外出しした
- **なぜ Runtime ごとに hook を分けたか**: 同じプロジェクトを手元の CLI とサーバの両方から触るとき、片方でしか走らせたくない hook (デスクトップ通知 vs. 監査ログ) が頻出するため

## 次に読むもの

- 柱を順に深掘り:
  - [イベントドリブンなワークフロー](event-driven-workflow.md)
  - [「次の 1 件」に集中できる実行モデル](task-decomposition.md)
  - [Contract による全体像の保持](contract.md)
- 実行基盤の使い分け → [Runtime の使い分け](runtimes.md)
