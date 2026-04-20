# 「次の 1 件」に集中できる実行モデル

> 3 つの柱のうち **柱 2**。[コアコンセプト: 3 つの柱](core-concept.md) で全体像を先に確認してから読んでください。

## 解きたい問題

AI エージェントに「このプロジェクトを全部進めて」と渡すのは典型的なアンチパターンです:

- **context の汚染**: 同じ session 内で関係ない調査結果・実装コード・テスト結果が混ざると、判断がぶれる
- **トークン溢れ**: 1 session で作業すれば context は単調に増え続ける。どこかで破綻する
- **並列化できない**: 「次はこれやって」「次はこれ」と逐次的に詰めるので、別 session に切り出せない

senko はこれを **Task という「1 context window で完結する粒度」** と、**依存解決・優先度に基づく自動ピック** で解きます。

## Task: 1 context window で完結する粒度

### 基本属性

```
task_number: プロジェクト内で一意な連番 (CLI が表示する ID)
title:       タスク名 (必須)
priority:    P0 〜 P3 (既定 P2、P0 が最優先)
status:      draft → todo → in_progress → completed / canceled
dependencies: 他タスクへの依存 (task_number の配列)
```

### 状態遷移 (forward only)

```
draft → todo → in_progress → completed
                    ↓
                canceled   (どの状態からでも)
```

- 遷移は一方向。`completed` から戻すことはできない
- キャンセルだけが任意状態から許される緊急出口
- 巻き戻したければ、新しいタスクとして再登録するのが原則

### 「1 context window で完結」とは

実務上の目安:

- 作業開始 (`task start`) からコミット・PR 作成・`task complete` までを 1 session で完結できる規模
- 典型的には 1〜2 ファイル、数十〜数百行の差分
- 調査タスク (コード読解だけして Contract Notes に書き戻す) もこの粒度
- **駄目な例**: "auth を全部リファクタ" ← これは Contract の粒度

なぜこの粒度に揃えるか: タスク完了後に **session を閉じて context をリセット** できるからです。次のタスクで context を汚染しない規律が保てます。

## <a id="dependency"></a>Dependency: 「B 完了まで A は start 不可」

### 基本

```
task A → task B  :  "A depends on B"  : B が completed になるまで A は start 不可
```

- Task → Task の **有向辺**
- 循環は CLI / API レベルで検出して拒否される
- 循環検出は新規追加・編集の両方で走る

### 編集

```bash
senko task deps add <task> --on <dep>
senko task deps remove <task> --on <dep>
senko task deps set <task> --on <dep1>,<dep2>  # 置き換え
senko task deps list <task>
senko graph                                     # テキスト (Mermaid) で可視化
```

### Contract の依存はない

Contract は依存関係を持ちません。Contract 間の順序付けが必要なら、タグや命名規則で運用する想定です (Contract は「何を達成したいか」であり、「いつ順番にやるか」ではないため)。

## ready と自動ピック

### ready の定義

**`status = todo` かつ、依存タスクがすべて `completed`** になった Task は **ready** と呼ばれ、start 可能な候補に入ります。

```
task #3 (todo, deps = [#1, #2])

  #1 completed, #2 completed   →  #3 は ready
  #1 completed, #2 in_progress →  #3 はまだ ready ではない
  #1 canceled,  #2 completed   →  #3 はまだ ready ではない
                                    (canceled は completed と見なさない)
```

### `senko task next` の選択アルゴリズム

ready 集合から **1 件だけ** を以下の順で決定します:

```
priority (P0 → P3 の昇順)
    └─ tie breaker: created_at (古い順)
        └─ tie breaker: id (昇順)
```

エージェントは「次に何をやるか考えない」「candidate を人間に聞かない」を徹底できます。この決定論性こそが **柱 2** の実装形です。

### Skill 経由の動き

`/senko` (引数なし) はこの `task next` を内部で呼び、選ばれたタスクを 1 件表示してそのまま作業に入ります:

```
/senko                 # 次の 1 件を自動ピックして start まで
/senko 3               # 明示的に ID 3 を指定 (ready でなければ warn)
```

## 並列 pick

ready なタスクが複数ある場合、**同時に別々の session / worktree から pick できます**。

```
ready: [#5 (P0), #8 (P1), #12 (P1)]

  developer A: /senko          → #5 に着手 (P0 最優先)
  developer B: /senko          → #8 に着手 (tie-break で古い順)
```

senko は `assignee_user_id` / `assignee_session_id` を使って **誰が / どの session がピックしたか** を記録します。pick 済みのタスクは他 session の `task next` 候補から除外されるので、二重着手は起きません。

チーム運用では:

- **1 人 1 worktree / 1 session** が基本
- `assignee_user_id` で「自分に関係ないタスクは候補から外す」フィルタも可能
- Claude Code の並列セッション (別 terminal) でも同じ仕組みで衝突なく pick できる

## Task vs Contract の使い分け

| 判断軸 | Task | Contract |
|---|---|---|
| 完結粒度 | 1 context window | 複数タスクを束ねる |
| 状態 | 明示的なステートマシン | 状態なし (DoD 全チェック = is_completed) |
| 依存関係 | あり | なし |
| 累積的な知見 | 基本的に残らない | Notes に書き戻す |
| 例 | "webhook handler を実装", "X 関数の命名を揃える" | "認証層を OIDC に移行", "監査機能を追加" |

Contract は **柱 3** の話題。→ [Contract による全体像の保持](contract.md)

## よくある分解パターン

### パターン A: 新機能追加

```
Contract: Implement webhook delivery
  └─ Task 1 (P1): 受信エンドポイントを axum で追加
  └─ Task 2 (P1, deps=[1]): 認証ミドルウェアを挿入
  └─ Task 3 (P2, deps=[1]): e2e テストを追加
  └─ Task 4 (P2, deps=[2,3]): ドキュメントを更新
```

Task 3 は Task 2 に依存しない (実装と独立にテストを書ける) ので、**2 と 3 は並列 pick 可能**。

### パターン B: リファクタリング

```
Contract: Refactor auth middleware
  └─ Task 1 (P1): 現行の挙動を特徴づける test を追加 (characterization test)
  └─ Task 2 (P0, deps=[1]): 新しい AuthProvider trait を定義
  └─ Task 3 (P1, deps=[2]): 既存実装を adapter として trait 準拠に直す
  └─ Task 4 (P2, deps=[3]): 古い実装を消す
```

リファクタは **依存チェーンが直列** になりがちで、並列 pick の恩恵は小さい。それでも 1 session に全部詰めないことに価値がある (context を切って観察しながら進められる)。

### パターン C: 調査

```
Contract: Investigate PostgreSQL migration path
  └─ Task 1 (P2): 現行の SQLite 固有 SQL の洗い出し → Contract notes に追記
  └─ Task 2 (P2): Postgres 向けマイグレーション方針を案出 → notes
  └─ Task 3 (P1, deps=[1,2]): 判断を Contract DoD にチェック
```

調査タスクは **実装を伴わない** が、Notes 経由で Contract に知見が累積するので次のタスクが厚みを持って始まります。

## 設計判断

- **なぜ `task next` は決定論か**: エージェントに「何が次か」を推論させないため。候補選びは機械的にしないと、似た優先度のタスクで迷走する
- **なぜ forward-only な state machine か**: completed を巻き戻せる設計にすると、hook (例: 監査ログ送信) の冪等性が崩れる。巻き戻したければ新規タスクで表現する
- **なぜ `canceled` を completed と同一視しないか**: 依存を解かない。キャンセルは「未達」。下流タスクが続けて進んでしまうのを防ぐ

## 次に読むもの

- Contract との関係 → [Contract による全体像の保持](contract.md)
- Task 生成・編集コマンド → [CLI リファレンス](../reference/cli.md)
- Task を跨ぐイベントでの hook 発火 → [イベントドリブンなワークフロー](event-driven-workflow.md)
