# `[server.remote.*]` Hook Examples

Practical patterns for hooks that fire while `senko serve` runs in direct mode.

Schema: [Hooks Reference](../../reference/hooks.md). Where they live in config: [`[server.*]` / `[backend.*]` / `[server.auth.*]` Config](../../reference/config/server-remote.md).

## Ship audit logs to logger / syslog

```toml
[server.remote.task_complete.hooks.audit]
command = 'jq -c "{ ts: .event.timestamp, actor: .user.name, task: .event.task.id, title: .event.task.title }" | logger -t senko-audit'
mode = "async"
on_failure = "warn"
```

To audit every action, duplicate this hook across actions — at minimum `task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel`.

## Emit to CloudWatch Logs

On Lambda, stdout already ends up in CloudWatch — no special hook needed. For EC2 / containers:

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

(Grant `logs:PutLogEvents` via IAM.)

## Notify Slack / Teams

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

## Emit monitoring metrics

Via Prometheus pushgateway:

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

On hosts with a DataDog / New Relic agent installed, hitting the agent's API is simpler.

## Pre-hook for external verification

Require external CI approval on task completion (**be careful — this is heavy**):

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

> `sync + pre + abort` blocks the state transition. Don't put timeout-prone work here; it's safer for the CI to call `senko task complete` from a webhook on its side.

## Contract integration: sync a new note to Confluence

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

## Hook Execution Logs

- The server writes hook execution results as JSON log lines on stdout (info / warn / error).
- `[log] hook_output = "both"` also streams the hook's own `stdout`/`stderr` to stdout.
- Under systemd: `journalctl -u senko -f --output=json-pretty`.
- On Lambda: logs go to CloudWatch automatically.

## Server vs. CLI: Where to Put a Hook

| Goal | Where |
|---|---|
| Notify only on a developer's machine (e.g. Slack via a personal webhook) | `[cli.*]` |
| Capture audit logs for everyone on the server | `[server.remote.*]` |
| Run the same hook in both places | Duplicate the definition under both (each runtime fires only its own section) |

The active runtime differs between "CLI-invoked" and "HTTP-API-invoked" operations, so **put operational logs and audits under `[server.remote.*]`** — if you only put them under `[cli.*]`, operations from clients that hit the API directly (bots, etc.) won't be logged.
