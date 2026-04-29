---
id: senko-skill-flow
title: senko skill のフロー（/senko add と /senko start <id>）
description: /senko add でタスク（とContract）を作るフロー、/senko start <id> でタスクを実行するフローの内部分岐を図示
tags:
  - senko
  - skill
  - workflow
  - claude-code
created_at: 2026-04-29
updated_at: 2026-04-29
---

## 概要

senko skill（`.claude/skills/senko/`）の主要 2 フロー — タスク追加 (`/senko add`) と
タスク実行 (`/senko start <id>`) — の内部分岐をまとめる。skill 内の実装が分散している
（`SKILL.md` ルーティング、`workflows/add-task.md`、`workflows/execute-task.md`、
`workflows/contract-terminal.md` など）ため、全体像を 1 枚で把握する用途。

> `/senko resume <id>` は `in_progress` の再開専用で、ここでは扱わない。

## `/senko add <description>` のフロー

```mermaid
flowchart TD
    A(["/senko add ..."]) --> Mode{"--simple ?"}
    Mode -- yes --> Simple["Simple mode<br/>task add --&gt; edit --&gt; publish<br/>※ 計画/レビュー無し"]
    Mode -- no --> P0["Phase 0<br/>narrative init で NID 発行"]

    P0 --> P1["Phase 1: Planning<br/>AskUserQuestion で Q&amp;A ループ<br/>+ append-decision / append-constraint"]
    P1 --> Split{"分割するか?"}

    Split -- 単一で進める --> P2S["Phase 2 単一<br/>senko task add"]
    Split -- 分割する --> P15["Phase 1.5<br/>Contract draft<br/>title / description / DoD / tags<br/>を AskUserQuestion で確定"]
    P15 --> P2M["Phase 2 分割<br/>1. contract add<br/>2. sub-task add 複数<br/>3. terminal task add<br/>4. 各 task を --contract で紐付け<br/>   terminal には contract-terminal タグ + 専用 DoD<br/>5. contract note add で split サマリ"]

    P2S --> P3["Phase 3: 依存関係"]
    P2M --> P3
    P3 -. 分割時 .-> P3T["terminal が全 sub-task に依存"]
    P3 --> P4["Phase 4 step 1-3<br/>title / desc / tag / priority / DoD 仕上げ"]

    P4 --> BR{"repo 操作を含む?"}
    BR -- no --> RV
    BR -- yes --> SETBR["branch_template を解決<br/>--&gt; senko task edit --branch"]
    SETBR --> RV{"どちらのレビュアー?"}

    RV -- 単一 --> R1["single-task-reviewer"]
    RV -- 分割 --> R2["task-contract-reviewer"]
    R1 --> V{"Verdict"}
    R2 --> V

    V -- PASS --> PUB["senko task publish"]
    V -- PASS_WITH_MINOR_FIXES --> MF["AskUserQuestion で修正承認<br/>--&gt; task/contract edit"] --> PUB
    V -- BLOCKING_FIXES_REQUIRED --> BF["修正適用<br/>--&gt; build-packet を全引数で再構築<br/>--&gt; 再レビュー"] --> V
    V -- SHOULD_SPLIT_TASK<br/>※単一のみ --> SS["ユーザー承認 --&gt; NID は維持<br/>--&gt; Phase 1.5 / Phase 2 分割パスへ"] --> P15
    V -- INSUFFICIENT_PACKET --> IP["narrative/packet 修復<br/>--&gt; 再 build-packet"] --> V

    PUB --> End(["終了"])
    Simple --> End
```

主な分岐:

- **`--simple`**: planning / reviewer / Phase 0 narrative を全部スキップ。
- **split 判定**（Phase 1 末）: split する場合のみ Contract と terminal task が生成される。
- **branch 設定**（Phase 4 step 4）: repo 操作の無いタスク（調査だけ等）は branch を付けない。
- **reviewer 種別**: Contract の有無で `single-task-reviewer` / `task-contract-reviewer` に分岐。
- **Verdict 5 種**:
  - `PASS` — そのまま publish
  - `PASS_WITH_MINOR_FIXES` — ユーザー承認の修正だけ当てて publish
  - `BLOCKING_FIXES_REQUIRED` — 修正 → packet 再構築 → 再レビューのループ
  - `SHOULD_SPLIT_TASK`（単一パスのみ） — NID を維持したまま Phase 1.5 / 分割 Phase 2 へ転回
  - `INSUFFICIENT_PACKET` — narrative / packet を修復して再ビルド

## `/senko start <id>`（タスク実行）のフロー

```mermaid
flowchart TD
    A(["/senko start &lt;id&gt;"]) --> FromNext{"senko task next<br/>からの遷移?"}
    FromNext -- yes --> S1
    FromNext -- no --> PG["Pre-check<br/>senko task get"]

    PG --> ST{"status ?"}
    ST -- todo --> DEP{"依存が全て<br/>completed ?"}
    ST -- draft --> X1["「publish が必要」と案内し停止"]
    ST -- in_progress --> X2["/senko resume を案内し停止"]
    ST -- completed/canceled --> X3["終了済みで停止"]
    DEP -- 未完あり --> X4["未完依存を提示して停止"]
    DEP -- ok --> META["task_start metadata 構築<br/>--&gt; senko task start"]

    META --> S1["Step 1: Review Task<br/>desc / plan / DoD / scope を読む"]
    S1 --> CC{"contract_id あり?"}
    CC -- yes --> LC["contract get +<br/>contract note list を全ページ走査"]
    CC -- no --> TT
    LC --> TT{"contract-terminal<br/>タグ?"}
    TT -- yes --> RD["contract-terminal.md へ転送<br/>※ 以降は実装ではなく Contract DoD 検証"]
    TT -- no --> S2{"branch 設定あり?"}

    S2 -- yes --> WT["worktree 作成（プロジェクトの手順に従う）"]
    S2 -- no --> S3
    WT --> S3["Step 3: Plan Mode<br/>EnterPlanMode<br/>+ generate-plan-sections.sh 出力を埋め込む"]
    S3 --> AP["ユーザーが plan を承認"]
    AP --> IMP["実装"]

    IMP --> NL{"contract_id あり?"}
    NL -- yes --> NOTES["随時 contract note add<br/>1 設計判断<br/>2 ハマり/落とし穴<br/>3 完了直前のサマリ"]
    NL -- no --> FIN(["Finalization へ"])
    NOTES --> FIN
```

主な分岐:

- **`task next` 経由かどうか**: 経由なら Pre-check / `task start` をスキップして即 Step 1 へ。
- **status / 依存ガード**: `todo` でかつ全依存 completed のときのみ実行可。それ以外は理由を出して停止（`in_progress` だけは `/senko resume` を案内）。
- **contract_id**: 設定されていれば Contract 本体と全 note を実行前に読み込み、実装中も note を追記。
- **`contract-terminal` タグ**: 以降の標準ワークフローを踏まず `contract-terminal.md`（Contract DoD 検証 + 不足分の follow-up タスク作成）に切り替わる。
- **branch**: 設定があれば worktree を切る（プロジェクトの手順に従う）。無ければ worktree 作成自体をスキップ。

## 参照

- `.claude/skills/senko/SKILL.md` — ルーティング規則
- `.claude/skills/senko/workflows/add-task.md` — `/senko add` のフェーズ詳細
- `.claude/skills/senko/workflows/execute-task.md` — `/senko start <id>` のステップ詳細
- `.claude/skills/senko/workflows/contract-terminal.md` — terminal task の検証ワークフロー
- `.claude/skills/senko/workflows/resume-task.md` — `/senko resume <id>` の再開ワークフロー
