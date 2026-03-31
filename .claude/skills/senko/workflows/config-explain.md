# Config Explain

Explain the current senko configuration values and their meanings.

## Procedure

### Step 1: Get Current Config

```bash
senko config
```

Parse the JSON output to extract all configuration sections.

### Step 2: Explain Each Section

For each section, explain every item's **current value**, whether it's the **default**, what it **means**, and the **available options**.

#### Sections

**workflow**
| Key | Default | Options | Description |
|---|---|---|---|
| `completion_mode` | `merge_then_complete` | `merge_then_complete`, `pr_then_complete` | Controls whether the branch is merged before or after task completion. `pr_then_complete` requires a PR URL and merge status check. |
| `auto_merge` | `true` | `true`, `false` | When `false` with `pr_then_complete`, the PR must also be approved before completion. |
| `branch_mode` | `worktree` | `worktree`, `branch` | How task branches are created. `worktree` uses git worktrees (parallel work), `branch` uses regular branches. |
| `merge_strategy` | `rebase` | `rebase`, `squash` | Git merge strategy when merging task branches back to main. |
| `events` | `[]` | list of event directives | Workflow event hooks (type: `command` or `prompt`) triggered at specific points (e.g., `pre_merge`, `post_pr`). |

**backend**
| Key | Default | Options | Description |
|---|---|---|---|
| `api_url` | `null` | URL string | HTTP backend API URL. When set, senko operates in remote mode. |
| `api_key` | `null` | string | API key for authenticating with the remote backend. |
| `hook_mode` | `server` | `server`, `client`, `both` | Where hooks execute: `server` (remote), `client` (local), or `both`. |

**storage**
| Key | Default | Options | Description |
|---|---|---|---|
| `db_path` | auto (`$XDG_DATA_HOME/senko/projects/<hash>/data.db`) | file path | Path to the SQLite database file. |

**log**
| Key | Default | Options | Description |
|---|---|---|---|
| `dir` | auto (`$XDG_STATE_HOME/senko`) | directory path | Directory for log files. |
| `level` | `info` | `trace`, `debug`, `info`, `warn`, `error` | Minimum log level. |
| `format` | `json` | `json`, `text` | Log output format. |

**project**
| Key | Default | Options | Description |
|---|---|---|---|
| `name` | `null` (auto-detected) | string | Project name. Used for hook environment variables and identification. |

**user**
| Key | Default | Options | Description |
|---|---|---|---|
| `name` | `null` (auto-detected) | string | User name for task assignment. |

**web**
| Key | Default | Options | Description |
|---|---|---|---|
| `host` | `127.0.0.1` | IP address | Host address for the web server. |

**hooks**
| Event | Description |
|---|---|
| `on_task_added` | Triggered when a new task is created. |
| `on_task_ready` | Triggered when a task moves to `todo` status. |
| `on_task_started` | Triggered when a task moves to `in_progress`. |
| `on_task_completed` | Triggered when a task is completed. |
| `on_task_canceled` | Triggered when a task is canceled. |
| `on_no_eligible_task` | Triggered when `senko next` finds no eligible tasks. |

Each hook entry has: `command` (shell command), `enabled` (bool, default true), `requires_env` (list of required env vars).

### Step 3: Explain Config Layering

Explain how configuration is resolved (highest priority first):
1. **CLI flags** (`--config <path>`)
2. **Environment variables** (`SENKO_*` — e.g., `SENKO_COMPLETION_MODE`, `SENKO_API_URL`)
3. **Project config** (`.senko/config.toml` in the project root)
4. **User config** (`~/.config/senko/config.toml`)

Higher-priority sources override lower ones. The `senko config` output shows the **merged** result.

### Step 4: Present to User

Format the explanation clearly, highlighting:
- Values that differ from defaults
- Any potentially important settings (e.g., `completion_mode`, `hook_mode`)
- Hooks that are currently configured
