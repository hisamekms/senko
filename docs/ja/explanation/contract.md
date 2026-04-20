# Contract による全体像の保持

> 3 つの柱のうち **柱 3**。[コアコンセプト: 3 つの柱](core-concept.md) で全体像を先に確認してから読んでください。

## 解きたい問題

[「次の 1 件」に集中できる実行モデル](task-decomposition.md) の方針に従うと、Task は **1 context window で完結する粒度** に揃えられます。しかし実作業は「1 機能追加」「1 リファクタ」「1 移行」のように **複数タスクをまたぐ単位** で起きるため、Task だけでは以下が取りこぼされます:

- **全体像**: そもそも何を達成したかったのか
- **累積する制約や設計判断**: タスク 1 で気付いた制約がタスク 3 で必要になる
- **横断的 DoD**: 「SIEM 連携まで含めて 1 機能」のような、個別タスクには収まらない完了条件
- **調査の産物**: 調査タスクが得た知見が、調査タスクの completion と同時に蒸発する

senko では **Contract** がこの粒度を担います。

## Contract の位置づけ

```
Contract (粗い粒度)
  │ 目的・全体 DoD・累積 Notes を保持
  │
  └─ Task (細かい粒度)
       │ 1 context window で完結
       │ 状態遷移は forward only
       │
       └─ Contract に Notes を書き戻す
          (source_task_id 付きで記録)
```

## Contract のフィールド

| フィールド | 型 | 説明 |
|---|---|---|
| `id` | int | Contract ID |
| `title` | string | Contract 名 (必須) |
| `description` | string? | 概要 |
| `definition_of_done` | `{content, checked}[]` | Contract レベルの完了条件 |
| `tags` | string[] | 分類 |
| `metadata` | JSON | 任意 (Project 単位の MetadataField で schema 化可) |
| `notes` | `{content, source_task_id, created_at}[]` | 作業中に得られた知見ログ |
| `created_at` / `updated_at` | ISO 8601 | タイムスタンプ |

**重要な非対称**: Contract には `status` がありません。代わりに、

```
is_completed = DoD が 1 件以上あり、かつすべて checked
```

で「完了かどうか」を導出します (DoD が空の Contract は is_completed にはなりません — 評価基準がないため)。

## Task との関係

Task は `contract_id` で **任意の 1 つの Contract にリンク** できます (optional)。

```bash
senko task add --title "Add webhook endpoint" --contract 7
senko task list --contract 7       # Contract 7 配下の Task を列挙
```

- 1 Task は最大 1 Contract にしか所属できない
- Task の状態遷移と Contract の DoD チェックは **独立** (Task 完了 ≠ Contract DoD チェック)

## Notes: 作業中の知見を書き戻す

### 基本

Notes は **タスクで得た知見を Contract に追記する** 仕組みです:

```bash
senko contract note add 7 \
  --content "Postgres migration requires RDS Proxy due to Lambda connection pooling" \
  --source-task 23
```

- `source_task_id` があるため、**どのタスクで得られた知見か** を後から辿れる
- Notes は append-only (後から編集・削除しない運用)
- タイムスタンプ付き

### なぜ Notes を Contract に書き戻すのか

Task は完了後に context が閉じられます。タスクで得た気付き (ライブラリの落とし穴、追加で必要になった依存、認識したリスク) は、そのまま放置すると **次のタスクを始めるエージェントには見えません**。

Contract に Notes として書き戻しておけば、`senko contract get 7` で Notes を読むだけで「これまでの累積知見」を 1 箇所で引けます。Claude Code skill は task execute の冒頭で Contract の Notes を自動で読み込むので、この仕組みが実効性を持ちます。

### Notes の書き方パターン

良い例:

```
"Postgres migration requires RDS Proxy due to Lambda connection pooling"
"Existing auth middleware stores session tokens in a way that fails SOC2 review — need to rewrite, not patch"
"DB migration must run before server rollout; coordinated deploy needed"
```

- 将来のタスクで判断に使える **事実・制約・決定**
- 1 Note = 1 観察 (複数混ぜない)

悪い例:

```
"タスク 23 完了"                 ← 情報が無い。task log を見ればいい
"頑張った"                       ← 知見ではない
"全部やった"                     ← 将来の判断材料にならない
```

## DoD: Contract レベルの完了条件

Task の DoD はそのタスク単位ですが、**Contract の DoD は複数タスクを横断する要件** を表現します。

例: "Implement webhook delivery" Contract の DoD

```
- [x] 受信エンドポイントが実装されている
- [x] 認証ミドルウェアが適用されている
- [x] e2e テストが通っている
- [ ] SIEM に送信ログが到達している (ops 確認済み)
- [ ] ドキュメントに手順が記載されている
```

これらは個別 Task の完了と別のタイミングでチェック/アンチェックされます:

```bash
senko contract dod check 7 4        # 4 番目の DoD 項目を checked に
senko contract dod uncheck 7 4      # 取り消し
```

`is_completed = true` になるまで、Contract は「まだ進行中」と扱うのが運用原則です。

## Contract が向く / 向かないケース

### 向くケース

- **複数タスクにまたがる機能追加**: 「webhook delivery を実装」
- **移行・リファクタ**: 「認証層を OIDC に移行」「auth middleware を rewrite」
- **調査プロジェクト**: 「Postgres 移行パスを調査」
- **横断的 DoD が明確**: 「SOC2 レビューに通す」

### 向かないケース

- **単発の小タスク**: 独立したバグ修正、コメント修正、typo fix — Contract なしで Task 単体でよい
- **継続的な保守**: 「常時 lint を通す」のような永続ルール — これは hook / workflow stage の領分
- **順序付き一連のタスク群**: Contract 同士に依存関係を持たせたいケース — senko は Contract 間の依存を表現しない (タグや命名規則で運用)

## Contract DoD と hook の連携

`contract_dod_check` / `contract_note_add` は他のイベントと同じく hook を発火できます:

```toml
[server.remote.contract_dod_check.hooks.audit]
command = "logger -t senko-audit 'contract DoD checked'"
mode = "async"

[workflow.contract_note_add.hooks.dedup]
command = "true"
prompt = "Skip the note if the same observation already exists in earlier notes."
when = "pre"
```

→ Hook の仕組み: [イベントドリブンなワークフロー](event-driven-workflow.md)

## 典型ライフサイクル

```
1. senko contract add --title "Migrate auth to OIDC" \
      --definition-of-done "Existing users can log in without disruption" \
      --definition-of-done "Legacy API keys are revoked"

2. senko task add --title "Add OIDC config skeleton" --contract 7
   senko task add --title "Wire JWT verifier" --contract 7
   senko task add --title "Migrate first internal service" --contract 7

3. 各 task を execute する過程で
   senko contract note add 7 --content "..." --source-task <id>

4. 全タスク完了後に Contract DoD をレビューし、
   満たせているものを senko contract dod check

5. is_completed = true になったら Contract 完了
```

## 設計判断

- **なぜ Contract に status が無いか**: 「何を達成したいか」の器なので、個別 Task のように一方向的に前に進むものではない。DoD が段階的に埋まる進行形の集約
- **なぜ Notes を append-only にしたか**: 書き戻した知見を後から編集・削除可能にすると、累積履歴としての信頼性が下がる。間違いを正したければ新しい Note を追記する運用
- **なぜ Task ↔ Contract は 1:N (Task は 1 Contract のみ) か**: Task は 1 粒度 = 1 Contract 所属に絞ることで、Notes の `source_task_id` が曖昧にならない

## 次に読むもの

- Task の粒度 → [「次の 1 件」に集中できる実行モデル](task-decomposition.md)
- Contract 関連のイベントと hook → [イベントドリブンなワークフロー](event-driven-workflow.md)
- CLI コマンドの詳細 → [CLI リファレンス](../reference/cli.md)
- DB スキーマ → [データモデル](../reference/data-model.md)
