use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::domain::task::CompletionMode;

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
    #[serde(default)]
    pub web: WebConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
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

#[cfg(feature = "postgres")]
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PostgresConfig {
    pub url: Option<String>,
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
    #[cfg(feature = "postgres")]
    #[serde(default)]
    pub postgres: Option<PostgresConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    #[default]
    Server,
    Client,
    Both,
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
    #[serde(default)]
    pub requires_env: Vec<String>,
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

    /// Get enabled entries with their names for a given event name.
    pub fn entries_for_event(&self, event_name: &str) -> Vec<(&str, &HookEntry)> {
        let map = match event_name {
            "task_added" => &self.on_task_added,
            "task_ready" => &self.on_task_ready,
            "task_started" => &self.on_task_started,
            "task_completed" => &self.on_task_completed,
            "task_canceled" => &self.on_task_canceled,
            "no_eligible_task" => &self.on_no_eligible_task,
            _ => return vec![],
        };
        map.iter()
            .filter(|(_, e)| e.enabled)
            .map(|(name, entry)| (name.as_str(), entry))
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
    #[serde(default)]
    pub web: RawWebConfig,
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
    #[cfg(feature = "postgres")]
    pub postgres: Option<PostgresConfig>,
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

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawWebConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
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
                #[cfg(feature = "postgres")]
                postgres: overlay.backend.postgres.or(self.backend.postgres),
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
            web: RawWebConfig {
                host: overlay.web.host.or(self.web.host),
                port: overlay.web.port.or(self.web.port),
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
                #[cfg(feature = "postgres")]
                postgres: self.backend.postgres,
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
            web: WebConfig {
                host: self.web.host,
                port: self.web.port,
            },
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

// --- CLI overrides ---

#[derive(Debug, Default)]
pub struct CliOverrides {
    pub log_dir: Option<String>,
    pub db_path: Option<String>,
    pub postgres_url: Option<String>,
    pub project: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub host: Option<String>,
}

impl Config {
    /// Apply environment variable overrides. Call after `RawConfig::resolve()`.
    /// Priority: env > config.toml defaults.
    pub fn apply_env(&mut self) {
        // Workflow settings
        if let Ok(val) = std::env::var("SENKO_COMPLETION_MODE") {
            match val.as_str() {
                "merge_then_complete" => {
                    self.workflow.completion_mode = CompletionMode::MergeThenComplete
                }
                "pr_then_complete" => {
                    self.workflow.completion_mode = CompletionMode::PrThenComplete
                }
                other => eprintln!("warning: unknown SENKO_COMPLETION_MODE={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_AUTO_MERGE") {
            match val.to_lowercase().as_str() {
                "true" | "1" | "yes" => self.workflow.auto_merge = true,
                "false" | "0" | "no" => self.workflow.auto_merge = false,
                other => eprintln!("warning: unknown SENKO_AUTO_MERGE={other}, ignoring"),
            }
        }

        // Backend settings
        if let Ok(val) = std::env::var("SENKO_API_URL") {
            if !val.is_empty() {
                self.backend.api_url = Some(val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_API_KEY") {
            if !val.is_empty() {
                self.backend.api_key = Some(val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_MODE") {
            match val.to_lowercase().as_str() {
                "server" => self.backend.hook_mode = HookMode::Server,
                "client" => self.backend.hook_mode = HookMode::Client,
                "both" => self.backend.hook_mode = HookMode::Both,
                other => eprintln!("warning: unknown SENKO_HOOK_MODE={other}, ignoring"),
            }
        }

        // DynamoDB settings (feature-gated)
        #[cfg(feature = "dynamodb")]
        {
            if let Ok(val) = std::env::var("SENKO_DYNAMODB_TABLE") {
                if !val.is_empty() {
                    self.backend
                        .dynamodb
                        .get_or_insert_with(DynamoDbConfig::default)
                        .table_name = Some(val);
                }
            }
            if let Ok(val) = std::env::var("SENKO_DYNAMODB_REGION") {
                if !val.is_empty() {
                    self.backend
                        .dynamodb
                        .get_or_insert_with(DynamoDbConfig::default)
                        .region = Some(val);
                }
            }
        }

        // PostgreSQL settings (feature-gated)
        #[cfg(feature = "postgres")]
        {
            if let Ok(val) = std::env::var("SENKO_POSTGRES_URL") {
                if !val.is_empty() {
                    self.backend
                        .postgres
                        .get_or_insert_with(PostgresConfig::default)
                        .url = Some(val);
                }
            }
        }

        // Hook commands (insert as named "_env" entry)
        fn insert_env_hook(map: &mut BTreeMap<String, HookEntry>, val: String) {
            map.insert(
                "_env".to_string(),
                HookEntry {
                    command: val,
                    enabled: true,
                    requires_env: vec![],
                },
            );
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_TASK_ADDED") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_task_added, val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_TASK_READY") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_task_ready, val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_TASK_STARTED") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_task_started, val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_TASK_COMPLETED") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_task_completed, val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_TASK_CANCELED") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_task_canceled, val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOOK_ON_NO_ELIGIBLE_TASK") {
            if !val.is_empty() {
                insert_env_hook(&mut self.hooks.on_no_eligible_task, val);
            }
        }

        // User settings
        if let Ok(val) = std::env::var("SENKO_USER") {
            if !val.is_empty() {
                self.user.name = Some(val);
            }
        }

        // Project settings
        if let Ok(val) = std::env::var("SENKO_PROJECT") {
            if !val.is_empty() {
                self.project.name = Some(val);
            }
        }

        // Storage settings
        if let Ok(val) = std::env::var("SENKO_DB_PATH") {
            if !val.is_empty() {
                self.storage.db_path = Some(val);
            }
        }

        // Log settings
        if let Ok(val) = std::env::var("SENKO_LOG_DIR") {
            if !val.is_empty() {
                self.log.dir = Some(val);
            }
        }
        if let Ok(val) = std::env::var("SENKO_LOG_LEVEL") {
            if !val.is_empty() {
                self.log.level = val;
            }
        }
        if let Ok(val) = std::env::var("SENKO_LOG_FORMAT") {
            match val.to_lowercase().as_str() {
                "json" => self.log.format = LogFormat::Json,
                "pretty" => self.log.format = LogFormat::Pretty,
                other => eprintln!("warning: unknown SENKO_LOG_FORMAT={other}, ignoring"),
            }
        }

        // Web settings
        if let Ok(val) = std::env::var("SENKO_PORT") {
            if let Ok(port) = val.parse::<u16>() {
                self.web.port = Some(port);
            }
        }
        if let Ok(val) = std::env::var("SENKO_HOST") {
            if !val.is_empty() {
                self.web.host = Some(val);
            }
        }
    }

    /// Apply CLI argument overrides. Call after `apply_env()`.
    /// Priority: CLI > env > config.toml > defaults.
    pub fn apply_cli(&mut self, overrides: &CliOverrides) {
        if let Some(ref dir) = overrides.log_dir {
            self.log.dir = Some(dir.clone());
        }
        if let Some(ref path) = overrides.db_path {
            self.storage.db_path = Some(path.clone());
        }
        #[cfg(feature = "postgres")]
        if let Some(ref url) = overrides.postgres_url {
            self.backend
                .postgres
                .get_or_insert_with(PostgresConfig::default)
                .url = Some(url.clone());
        }
        if let Some(ref name) = overrides.project {
            self.project.name = Some(name.clone());
        }
        if let Some(ref name) = overrides.user {
            self.user.name = Some(name.clone());
        }
        if let Some(port) = overrides.port {
            self.web.port = Some(port);
        }
        if let Some(ref host) = overrides.host {
            self.web.host = Some(host.clone());
        }
    }

    pub fn web_port_or(&self, default: u16) -> u16 {
        self.web.port.unwrap_or(default)
    }

    pub fn web_port_is_explicit(&self) -> bool {
        self.web.port.is_some()
    }

    pub fn effective_host(&self) -> String {
        self.web
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }
}
