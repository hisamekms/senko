# Workflow stage の実例

Claude Code の動作を、プロジェクト固有のルールに合わせて調整するための実践例集。

stage の概念は [explanation/event-driven-workflow.md](../../explanation/event-driven-workflow.md)、TOML スキーマは [reference/config/workflow.md](../../reference/config/workflow.md)。

## パターン 1: タスク追加時のデフォルト

新規タスクに毎回付けたいタグ・DoD・priority がある場合:

```toml
[workflow.task_add]
default_dod = [
  "Unit tests added",
  "CHANGELOG.md updated",
]
default_tags = ["backend"]
default_priority = "p2"
instructions = [
  "Acceptance Criteria を description に明記する",
  "見積が大きい (> 3 days) 場合は分割する",
]
```

CLI から直接 `senko task add` した時はこれらが自動で入ります。skill 経由だと Claude がこれを加味して問いかけます。

## パターン 2: plan stage に必須セクションを課す

設計フォーマットを統一したい場合:

```toml
[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria", "Risks"]
instructions = [
  "plan は task.plan フィールドに保存する",
  "Overview は 3 文以内",
  "Risks が 1 件もない plan は却下する",
]
```

skill は plan 生成時にこれを読み込み、不足があればエージェントに再作成を促します。

## パターン 3: branch_set でテンプレ統一

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"
branch_mode = "worktree"

[workflow.branch_set]
instructions = [
  "feature/ / fix/ prefix は使わない (branch_template で統一済)",
  "既存 worktree があるか先に確認する",
]
```

## パターン 4: task_complete で CI 通過を必須に

```toml
[workflow]
merge_via = "pr"

[workflow.task_complete.hooks.ci_green]
command = "gh pr checks $(cat) --required"
when = "pre"
mode = "sync"
on_failure = "abort"

[[workflow.task_complete.hooks.ci_green.env_vars]]
name = "SENKO_PR_URL"
required = true
```

**注**: hook は stdin に envelope JSON を受け取るので、`command` 側で `jq` してもよい:

```toml
command = "jq -r '.event.task.pr_url' | xargs -I{} gh pr checks {} --required"
```

## パターン 5: contract_note_add で重複を抑制

Contract に同じ知見を何度も書かせないように:

```toml
[workflow.contract_note_add.hooks.dedup_check]
command = "true"
prompt = "既存の notes を読み返し、同じ観察が既にあれば追記をスキップせよ"
when = "pre"
```

`command = "true"` で shell 側は no-op、`prompt` で Claude に指示を注入するパターン。

## パターン 6: plan stage で必須 metadata を回収

Project の metadata_field に `estimate_points` を `required_on_complete = true` で定義した前提:

```toml
[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "フィボナッチ数列 (1,2,3,5,8,13,21) で見積もる"
```

plan 終了時に metadata に注入されるので、complete 時の検証で弾かれない。

## パターン 7: 独自 stage を足す (senko skill は触れないが外部 script から読める)

```toml
[workflow.security_review]
instructions = [
  "変更箇所に credential / secrets の扱いがあれば SRE に相談する",
]
```

この stage は senko skill の組み込みフローでは発火しませんが、`senko config --output json` で取得できるので、独自の別 skill や CI script から参照できます。

## 検証

設定を書いた後は:

```bash
senko config                # マージ済み設定を確認
senko doctor                # mismatched runtime や invalid な hook 組合せを警告
senko hooks test task_complete 1    # hook を実発火させてテスト
```

## アンチパターン

- **instructions にコード規約を大量に詰め込む** → エージェントの応答速度が落ち、指示が守られにくい。規約は `docs/` にまとめて、instructions では「`docs/code-style.md` を読んでから実装」と書く方が効く
- **hook に長時間の処理を置く (`when = "pre"` + `mode = "sync"` で数分)** → CLI が固まる。async にするか job queue を噛ませる
- **`command = "..."` に secrets 直書き** → `env_vars` で `required = true` を使って CI secret 経由で注入する
