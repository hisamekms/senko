# `[cli.*]` hook の実例

ローカル CLI で動かす時に有効な hook の実践パターン集。

スキーマは [Hooks リファレンス](../../reference/hooks.md)、置き場所は [`[cli.*]` 設定](../../reference/config/cli.md)。

## デスクトップ通知

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'senko' 'task completed'"
mode = "async"
on_failure = "ignore"
```

## タスク開始時にブランチ名をクリップボードへ

```toml
[cli.task_start.hooks.copy_branch]
command = "jq -r '.event.task.branch' | pbcopy"   # macOS
mode = "sync"
on_failure = "warn"
```

stdin に hook envelope JSON が来るので `jq` で切り出す。Linux なら `xclip -selection clipboard` / `wl-copy` 等に変更。

## Slack に通知

```toml
[cli.task_complete.hooks.slack]
command = 'jq -c "{text: (\"✅ \" + .event.task.title)}" | curl -s -X POST -H "Content-Type: application/json" -d @- "$SLACK_WEBHOOK_URL"'
mode = "async"

[[cli.task_complete.hooks.slack.env_vars]]
name = "SLACK_WEBHOOK_URL"
required = true
description = "Slack Incoming Webhook URL"
```

`SLACK_WEBHOOK_URL` が未設定ならこの hook はスキップ + warn。

## タスク開始で計測を開始

```toml
[cli.task_start.hooks.start_timer]
command = "date +%s > /tmp/senko-task-start"
mode = "sync"

[cli.task_complete.hooks.report_elapsed]
command = '''
START=$(cat /tmp/senko-task-start 2>/dev/null || echo 0)
NOW=$(date +%s)
echo "elapsed: $((NOW - START))s"
'''
mode = "sync"
```

## ready タスクが 0 の時に知らせる

```toml
[cli.task_select.hooks.nothing_ready]
command = "notify-send 'senko' 'No ready tasks — add one?'"
on_result = "none"
mode = "async"
```

`on_result = "none"` は task_select でのみ有効。タスクが選ばれなかったケースに限定。

## Pre-hook でファイル編集を防ぐ (実例)

変なタイミングで `task complete` を叩かないよう、作業ブランチと現在ブランチが一致しないと complete を拒否する:

```toml
[cli.task_complete.hooks.branch_guard]
command = '''
EXPECTED=$(jq -r '.event.task.branch // empty')
CURRENT=$(git rev-parse --abbrev-ref HEAD)
if [ -n "$EXPECTED" ] && [ "$EXPECTED" != "$CURRENT" ]; then
  echo "not on task branch: expected=$EXPECTED current=$CURRENT" >&2
  exit 1
fi
'''
when = "pre"
mode = "sync"
on_failure = "abort"    # sync + pre でのみ abort が効く
```

## 複数 hook を同じ action に並べる

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'senko' 'done'"
mode = "async"

[cli.task_complete.hooks.log]
command = "echo done >> /tmp/senko.log"
mode = "async"

[cli.task_complete.hooks.webhook]
command = "curl -X POST $WEBHOOK_URL"
mode = "async"
[[cli.task_complete.hooks.webhook.env_vars]]
name = "WEBHOOK_URL"
required = false
default = "http://127.0.0.1:8080/hook"
```

hook は **発火順が保証されません** (並列 spawn)。順序が重要なら 1 つの command 内で逐次実行。

## 一時的に無効化

hook を消さず `enabled = false` で止める:

```toml
[cli.task_complete.hooks.slack]
command = "..."
enabled = false   # 復活させたいときは true に戻すだけ
```

## デバッグ

```bash
senko hooks test task_complete 3         # 実 task 3 で hook を同期発火
senko hooks test task_complete --dry-run # envelope だけ表示

senko hooks log -n 50                     # 直近 50 件
senko hooks log -f                        # tail -f 相当
senko --log-dir /tmp/senko-logs task ...  # 一時的に別ディレクトリにログ
```

hook の `stdout`/`stderr` をコンソールに出したい場合:

```toml
[log]
hook_output = "both"    # file に書きつつ stdout にも流す
```
