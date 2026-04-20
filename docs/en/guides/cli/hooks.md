# `[cli.*]` Hook Examples

Practical patterns for hooks that fire when running locally under the CLI runtime.

Schema: [Hooks Reference](../../reference/hooks.md). Where they live in config: [`[cli.*]` Config](../../reference/config/cli.md).

## Desktop notification

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'senko' 'task completed'"
mode = "async"
on_failure = "ignore"
```

## Copy the branch name to clipboard on task start

```toml
[cli.task_start.hooks.copy_branch]
command = "jq -r '.event.task.branch' | pbcopy"   # macOS
mode = "sync"
on_failure = "warn"
```

The hook envelope arrives on stdin, so you extract fields with `jq`. Swap `pbcopy` for `xclip -selection clipboard` or `wl-copy` on Linux.

## Post to Slack

```toml
[cli.task_complete.hooks.slack]
command = 'jq -c "{text: (\"✅ \" + .event.task.title)}" | curl -s -X POST -H "Content-Type: application/json" -d @- "$SLACK_WEBHOOK_URL"'
mode = "async"

[[cli.task_complete.hooks.slack.env_vars]]
name = "SLACK_WEBHOOK_URL"
required = true
description = "Slack Incoming Webhook URL"
```

If `SLACK_WEBHOOK_URL` isn't set, the hook is skipped with a warn.

## Start a timer on task start

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

## Notify when there are no ready tasks

```toml
[cli.task_select.hooks.nothing_ready]
command = "notify-send 'senko' 'No ready tasks — add one?'"
on_result = "none"
mode = "async"
```

`on_result = "none"` is valid only for `task_select`. It fires only when no task was picked.

## Pre-hook that blocks the transition

Reject `task complete` when the working branch doesn't match the task's branch:

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
on_failure = "abort"    # abort only works with sync + pre
```

## Multiple hooks on the same action

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

Hooks **are not fired in a guaranteed order** (parallel spawn). If order matters, run them sequentially inside a single `command`.

## Disabling temporarily

Keep the hook but pause it via `enabled = false`:

```toml
[cli.task_complete.hooks.slack]
command = "..."
enabled = false   # flip back to true to re-enable
```

## Debugging

```bash
senko hooks test task_complete 3         # fire the hook synchronously against real task 3
senko hooks test task_complete --dry-run # show the envelope only

senko hooks log -n 50                     # last 50 entries
senko hooks log -f                        # tail -f equivalent
senko --log-dir /tmp/senko-logs task ...  # redirect logs to a temporary directory
```

To send hook `stdout`/`stderr` to the console:

```toml
[log]
hook_output = "both"    # write to file AND stream to stdout
```
