# `[server.remote.*]` hook の実例

`senko serve` (direct モード) で動作中に発火する hook の実践パターン。

スキーマは [Hooks リファレンス](../../reference/hooks.md)、置き場所は [`[server.*]` / `[backend.*]` / `[server.auth.*]` 設定](../../reference/config/server-remote.md)。

## 監査ログを logger / syslog に流す

```toml
[server.remote.task_complete.hooks.audit]
command = 'jq -c "{ ts: .event.timestamp, actor: .user.name, task: .event.task.id, title: .event.task.title }" | logger -t senko-audit'
mode = "async"
on_failure = "warn"
```

全 action を監査したいなら同じ hook を各 action に複製、または `task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` のすべてに展開。

## CloudWatch Logs に emit

Lambda 配下なら stdout に出せば CloudWatch に行くので、特別な hook は不要。
EC2 / container なら:

```toml
[server.remote.task_complete.hooks.cloudwatch]
command = '''
aws logs put-log-events \
  --log-group-name /senko/audit \
  --log-stream-name $(hostname) \
  --log-events timestamp=$(date +%s000),message="$(jq -c .)"
'''
mode = "async"
```

(IAM で `logs:PutLogEvents` を許可)

## Slack / Teams に通知

```toml
[server.remote.task_complete.hooks.slack]
command = '''
jq -c '{text: ("✅ " + .event.task.title + " by " + .user.name)}' \
  | curl -s -X POST -H "Content-Type: application/json" -d @- "$SLACK_WEBHOOK_URL"
'''
mode = "async"

[[server.remote.task_complete.hooks.slack.env_vars]]
name = "SLACK_WEBHOOK_URL"
required = true
```

## 監視メトリクスを emit

Prometheus pushgateway 経由:

```toml
[server.remote.task_complete.hooks.metrics]
command = '''
COUNT=$(jq -r ".event.stats.completed")
PROJECT=$(jq -r ".project.name")
curl -s --data "senko_completed_total{project=\"$PROJECT\"} $COUNT" \
  "$PUSHGATEWAY_URL/metrics/job/senko/instance/$(hostname)"
'''
mode = "async"

[[server.remote.task_complete.hooks.metrics.env_vars]]
name = "PUSHGATEWAY_URL"
required = true
```

DataDog / New Relic 等の agent が入っているホストなら agent の API を叩く方が簡単です。

## Pre-hook で外部検証を挟む

タスク完了時に外部 CI の承認を必須にしたい場合 (**重処理なので慎重に**):

```toml
[server.remote.task_complete.hooks.ci_gate]
command = '''
TASK_ID=$(jq -r ".event.task.id")
gh pr checks "$(jq -r ".event.task.pr_url")" --required
'''
when = "pre"
mode = "sync"
on_failure = "abort"
```

> `sync + pre + abort` は状態遷移をブロックします。タイムアウトしやすい処理はここに置かず、CI 側からの webhook で `senko task complete` を叩く方が安全。

## Contract 連携: note 追加時に Confluence へ同期

```toml
[server.remote.contract_note_add.hooks.confluence]
command = '''
CONTRACT=$(jq -r ".event.contract.title")
NOTE=$(jq -r ".event.contract.notes[-1].content")
curl -s -X POST "$CONFLUENCE_API/content" \
  -H "Authorization: Bearer $CONFLUENCE_TOKEN" \
  -H "Content-Type: application/json" \
  -d "$(jq -n --arg t "$CONTRACT" --arg n "$NOTE" '{title: $t, body: {storage: {value: $n, representation: "storage"}}}')"
'''
mode = "async"

[[server.remote.contract_note_add.hooks.confluence.env_vars]]
name = "CONFLUENCE_API"
required = true
[[server.remote.contract_note_add.hooks.confluence.env_vars]]
name = "CONFLUENCE_TOKEN"
required = true
```

## Hook 実行のログ

- サーバの stdout に JSON ログで hook 実行結果が出力される (info / warn / error)
- `[log] hook_output = "both"` にすると hook 自身の stdout/stderr も stdout に流れる
- systemd 配下なら `journalctl -u senko -f --output=json-pretty` で読める
- Lambda 配下なら CloudWatch Logs に自動で行く

## サーバ側 hook と CLI 側 hook の使い分け

| やりたいこと | どこに書く |
|---|---|
| クライアント端末でだけ通知する (Slack on 開発者の個人 webhook) | `[cli.*]` |
| サーバで全員分の監査ログを取る | `[server.remote.*]` |
| どちらも同じ hook を走らせたい | 両方に同じ定義を置く (片方ランタイムでしか発火しない) |

同じリソース操作でも CLI 経由 / HTTP API 経由で発火する runtime が変わるので、**運用ログや監査は必ず `[server.remote.*]` 側に置く** こと。CLI 側に置くと、サーバに直接 API を叩くクライアント (bot 等) の操作がログに残りません。
