use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub workflow: WorkflowConfig,
    #[serde(default)]
    pub backend: BackendConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub user: UserConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserConfig {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogConfig {
    pub dir: Option<String>,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub format: LogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

fn default_log_level() -> String {
    "info".to_string()
}

#[cfg(feature = "dynamodb")]
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DynamoDbConfig {
    pub table_name: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub hook_mode: HookMode,
    #[cfg(feature = "dynamodb")]
    #[serde(default)]
    pub dynamodb: Option<DynamoDbConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    #[default]
    Server,
    Client,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMode {
    MergeThenComplete,
    PrThenComplete,
}

impl Default for CompletionMode {
    fn default() -> Self {
        CompletionMode::MergeThenComplete
    }
}

impl std::fmt::Display for CompletionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompletionMode::MergeThenComplete => write!(f, "merge_then_complete"),
            CompletionMode::PrThenComplete => write!(f, "pr_then_complete"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub completion_mode: CompletionMode,
    #[serde(default = "default_true")]
    pub auto_merge: bool,
}

fn default_true() -> bool {
    true
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            completion_mode: CompletionMode::default(),
            auto_merge: true,
        }
    }
}

// --- Named hook types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub on_task_added: BTreeMap<String, HookEntry>,
    #[serde(default)]
    pub on_task_ready: BTreeMap<String, HookEntry>,
    #[serde(default)]
    pub on_task_started: BTreeMap<String, HookEntry>,
    #[serde(default)]
    pub on_task_completed: BTreeMap<String, HookEntry>,
    #[serde(default)]
    pub on_task_canceled: BTreeMap<String, HookEntry>,
    #[serde(default)]
    pub on_no_eligible_task: BTreeMap<String, HookEntry>,
}

impl HooksConfig {
    /// Get enabled commands for a given event name.
    pub fn commands_for_event(&self, event_name: &str) -> Vec<&str> {
        let map = match event_name {
            "task_added" => &self.on_task_added,
            "task_ready" => &self.on_task_ready,
            "task_started" => &self.on_task_started,
            "task_completed" => &self.on_task_completed,
            "task_canceled" => &self.on_task_canceled,
            "no_eligible_task" => &self.on_no_eligible_task,
            _ => return vec![],
        };
        map.values()
            .filter(|e| e.enabled)
            .map(|e| e.command.as_str())
            .collect()
    }
}

// --- RawConfig for layered merging ---

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawConfig {
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub workflow: RawWorkflowConfig,
    #[serde(default)]
    pub backend: RawBackendConfig,
    #[serde(default)]
    pub log: RawLogConfig,
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub user: UserConfig,
    #[serde(default)]
    pub auth: RawAuthConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawWorkflowConfig {
    pub completion_mode: Option<CompletionMode>,
    pub auto_merge: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawBackendConfig {
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub hook_mode: Option<HookMode>,
    #[cfg(feature = "dynamodb")]
    pub dynamodb: Option<DynamoDbConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawLogConfig {
    pub dir: Option<String>,
    pub level: Option<String>,
    pub format: Option<LogFormat>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawAuthConfig {
    pub enabled: Option<bool>,
}

impl RawConfig {
    /// Merge two configs: `self` is the base (lower priority), `overlay` wins.
    pub fn merge(self, overlay: RawConfig) -> RawConfig {
        RawConfig {
            hooks: merge_hooks(self.hooks, overlay.hooks),
            workflow: RawWorkflowConfig {
                completion_mode: overlay.workflow.completion_mode.or(self.workflow.completion_mode),
                auto_merge: overlay.workflow.auto_merge.or(self.workflow.auto_merge),
            },
            backend: RawBackendConfig {
                api_url: overlay.backend.api_url.or(self.backend.api_url),
                api_key: overlay.backend.api_key.or(self.backend.api_key),
                hook_mode: overlay.backend.hook_mode.or(self.backend.hook_mode),
                #[cfg(feature = "dynamodb")]
                dynamodb: overlay.backend.dynamodb.or(self.backend.dynamodb),
            },
            log: RawLogConfig {
                dir: overlay.log.dir.or(self.log.dir),
                level: overlay.log.level.or(self.log.level),
                format: overlay.log.format.or(self.log.format),
            },
            project: ProjectConfig {
                name: overlay.project.name.or(self.project.name),
            },
            user: UserConfig {
                name: overlay.user.name.or(self.user.name),
            },
            auth: RawAuthConfig {
                enabled: overlay.auth.enabled.or(self.auth.enabled),
            },
            storage: StorageConfig {
                db_path: overlay.storage.db_path.or(self.storage.db_path),
            },
        }
    }

    /// Resolve to final Config, filling None values with defaults.
    pub fn resolve(self) -> Config {
        Config {
            hooks: self.hooks,
            workflow: WorkflowConfig {
                completion_mode: self.workflow.completion_mode.unwrap_or_default(),
                auto_merge: self.workflow.auto_merge.unwrap_or(true),
            },
            backend: BackendConfig {
                api_url: self.backend.api_url,
                api_key: self.backend.api_key,
                hook_mode: self.backend.hook_mode.unwrap_or_default(),
                #[cfg(feature = "dynamodb")]
                dynamodb: self.backend.dynamodb,
            },
            log: LogConfig {
                dir: self.log.dir,
                level: self.log.level.unwrap_or_else(default_log_level),
                format: self.log.format.unwrap_or_default(),
            },
            project: self.project,
            user: self.user,
            auth: AuthConfig {
                enabled: self.auth.enabled.unwrap_or(false),
            },
            storage: self.storage,
        }
    }
}

/// Merge hooks: base hooks + overlay hooks. Same-name hooks: overlay wins.
/// Disabled hooks (enabled=false) are kept in the map (filtered at execution time).
fn merge_hooks(base: HooksConfig, overlay: HooksConfig) -> HooksConfig {
    fn merge_map(
        mut base: BTreeMap<String, HookEntry>,
        overlay: BTreeMap<String, HookEntry>,
    ) -> BTreeMap<String, HookEntry> {
        for (name, entry) in overlay {
            base.insert(name, entry);
        }
        base
    }
    HooksConfig {
        on_task_added: merge_map(base.on_task_added, overlay.on_task_added),
        on_task_ready: merge_map(base.on_task_ready, overlay.on_task_ready),
        on_task_started: merge_map(base.on_task_started, overlay.on_task_started),
        on_task_completed: merge_map(base.on_task_completed, overlay.on_task_completed),
        on_task_canceled: merge_map(base.on_task_canceled, overlay.on_task_canceled),
        on_no_eligible_task: merge_map(base.on_no_eligible_task, overlay.on_no_eligible_task),
    }
}
