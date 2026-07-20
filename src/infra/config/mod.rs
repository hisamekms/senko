use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

pub use crate::domain::task::{BranchMode, BranchModeType, MergeStrategy, MergeVia};
use crate::infra::xdg::XdgDirs;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
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
    pub server: ServerConfig,
    #[serde(default)]
    pub cli: CliConfig,
    /// Resolved XDG directories. Not serialized; populated at runtime by
    /// `bootstrap::load_config`. Tests and programmatic Config construction
    /// can leave this at `XdgDirs::default()` when XDG paths are not needed.
    #[serde(skip)]
    pub xdg: XdgDirs,
}

// --- Hook definition types (new unified schema) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    #[default]
    Abort,
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookWhen {
    Pre,
    #[default]
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    Sync,
    #[default]
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnResult {
    Selected,
    None,
    #[default]
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVarSpec {
    pub name: String,
    #[serde(default = "default_true")]
    pub required: bool,
    pub default: Option<String>,
    pub description: Option<String>,
}

/// Definition of a single hook. Used by every runtime (CLI / server / workflow).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    pub command: String,
    #[serde(default)]
    pub when: HookWhen,
    #[serde(default)]
    pub mode: HookMode,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub env_vars: Vec<EnvVarSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_result: Option<OnResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Per-hook timeout in seconds. Defaults to 30s. When the child process
    /// has not exited within this window, `run_hook_command` calls
    /// `Child::kill()` and emits `senko.hook.failed` with
    /// `failure.reason = "timeout"`.
    #[serde(default = "default_hook_timeout_secs")]
    pub timeout_secs: u64,
}

/// Map of hook name -> hook definition, used under each action / stage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionConfig {
    #[serde(default)]
    pub hooks: HashMap<String, HookDef>,
}

/// CLI / server runtimes expose a fixed set of task-aggregate actions.
/// Any action not listed here cannot carry hooks at the CLI/server level.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskActionHooks {
    #[serde(default)]
    pub task_add: ActionConfig,
    #[serde(default)]
    pub task_update: ActionConfig,
    #[serde(default)]
    pub task_publish: ActionConfig,
    #[serde(default)]
    pub task_start: ActionConfig,
    #[serde(default)]
    pub task_resume: ActionConfig,
    #[serde(default)]
    pub task_complete: ActionConfig,
    #[serde(default)]
    pub task_cancel: ActionConfig,
    #[serde(default)]
    pub task_select: ActionConfig,
}

impl TaskActionHooks {
    pub fn action_config(&self, action: &str) -> Option<&ActionConfig> {
        match action {
            "task_add" => Some(&self.task_add),
            "task_update" => Some(&self.task_update),
            "task_publish" => Some(&self.task_publish),
            "task_start" => Some(&self.task_start),
            "task_resume" => Some(&self.task_resume),
            "task_complete" => Some(&self.task_complete),
            "task_cancel" => Some(&self.task_cancel),
            "task_select" => Some(&self.task_select),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.task_add.hooks.is_empty()
            && self.task_update.hooks.is_empty()
            && self.task_publish.hooks.is_empty()
            && self.task_start.hooks.is_empty()
            && self.task_resume.hooks.is_empty()
            && self.task_complete.hooks.is_empty()
            && self.task_cancel.hooks.is_empty()
            && self.task_select.hooks.is_empty()
    }
}

/// CLI / server runtimes expose a fixed set of contract-aggregate actions.
/// Mirrors `TaskActionHooks` but for contract write operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContractActionHooks {
    #[serde(default)]
    pub contract_add: ActionConfig,
    #[serde(default)]
    pub contract_edit: ActionConfig,
    #[serde(default)]
    pub contract_delete: ActionConfig,
    #[serde(default)]
    pub contract_dod_check: ActionConfig,
    #[serde(default)]
    pub contract_dod_uncheck: ActionConfig,
    #[serde(default)]
    pub contract_note_add: ActionConfig,
}

impl ContractActionHooks {
    pub fn action_config(&self, action: &str) -> Option<&ActionConfig> {
        match action {
            "contract_add" => Some(&self.contract_add),
            "contract_edit" => Some(&self.contract_edit),
            "contract_delete" => Some(&self.contract_delete),
            "contract_dod_check" => Some(&self.contract_dod_check),
            "contract_dod_uncheck" => Some(&self.contract_dod_uncheck),
            "contract_note_add" => Some(&self.contract_note_add),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.contract_add.hooks.is_empty()
            && self.contract_edit.hooks.is_empty()
            && self.contract_delete.hooks.is_empty()
            && self.contract_dod_check.hooks.is_empty()
            && self.contract_dod_uncheck.hooks.is_empty()
            && self.contract_note_add.hooks.is_empty()
    }
}

/// CLI / server runtimes expose project-aggregate actions for hook config.
/// Mirrors `TaskActionHooks` but for project / project-member mutations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectActionHooks {
    #[serde(default)]
    pub project_create: ActionConfig,
    #[serde(default)]
    pub project_update: ActionConfig,
    #[serde(default)]
    pub project_member_add: ActionConfig,
    #[serde(default)]
    pub project_member_remove: ActionConfig,
    #[serde(default)]
    pub project_member_role_change: ActionConfig,
}

impl ProjectActionHooks {
    pub fn action_config(&self, action: &str) -> Option<&ActionConfig> {
        match action {
            "project_create" => Some(&self.project_create),
            "project_update" => Some(&self.project_update),
            "project_member_add" => Some(&self.project_member_add),
            "project_member_remove" => Some(&self.project_member_remove),
            "project_member_role_change" => Some(&self.project_member_role_change),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.project_create.hooks.is_empty()
            && self.project_update.hooks.is_empty()
            && self.project_member_add.hooks.is_empty()
            && self.project_member_remove.hooks.is_empty()
            && self.project_member_role_change.hooks.is_empty()
    }
}

/// CLI / server runtimes expose user-aggregate actions for hook config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserActionHooks {
    #[serde(default)]
    pub user_create: ActionConfig,
    #[serde(default)]
    pub user_update: ActionConfig,
    #[serde(default)]
    pub user_api_key_issue: ActionConfig,
    #[serde(default)]
    pub user_api_key_revoke: ActionConfig,
    #[serde(default)]
    pub user_session_revoke: ActionConfig,
}

impl UserActionHooks {
    pub fn action_config(&self, action: &str) -> Option<&ActionConfig> {
        match action {
            "user_create" => Some(&self.user_create),
            "user_update" => Some(&self.user_update),
            "user_api_key_issue" => Some(&self.user_api_key_issue),
            "user_api_key_revoke" => Some(&self.user_api_key_revoke),
            "user_session_revoke" => Some(&self.user_session_revoke),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.user_create.hooks.is_empty()
            && self.user_update.hooks.is_empty()
            && self.user_api_key_issue.hooks.is_empty()
            && self.user_api_key_revoke.hooks.is_empty()
            && self.user_session_revoke.hooks.is_empty()
    }
}

/// CLI / server runtimes expose metadata-field-aggregate actions for hook config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetadataFieldActionHooks {
    #[serde(default)]
    pub metadata_field_define: ActionConfig,
    #[serde(default)]
    pub metadata_field_remove: ActionConfig,
}

impl MetadataFieldActionHooks {
    pub fn action_config(&self, action: &str) -> Option<&ActionConfig> {
        match action {
            "metadata_field_define" => Some(&self.metadata_field_define),
            "metadata_field_remove" => Some(&self.metadata_field_remove),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.metadata_field_define.hooks.is_empty() && self.metadata_field_remove.hooks.is_empty()
    }
}

// --- Metadata field types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum MetadataFieldSource {
    Env { env_var: String },
    Prompt { prompt: String },
    Value { value: serde_json::Value },
    Command { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataField {
    pub key: String,
    #[serde(flatten)]
    pub source: MetadataFieldSource,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub required: bool,
}

// --- Workflow stage config (user-extensible) ---

/// A single workflow stage configuration. The set of stages is open-ended —
/// skill-provided stages (task_add / task_start / branch_create / etc.) are
/// merely conventions. Any additional fields (default_dod, required_sections,
/// etc.) are captured in `extra` and surfaced transparently via `senko config`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowStageConfig {
    #[serde(default)]
    pub metadata_fields: Vec<MetadataField>,
    #[serde(default)]
    pub instructions: Vec<String>,
    #[serde(default)]
    pub hooks: HashMap<String, HookDef>,
    /// Catch-all for stage-specific fields consumed by the skill or user scripts.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl WorkflowStageConfig {
    /// Attempt to decode a named extra field into the given type.
    pub fn stage_field<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.extra
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

// --- CLI config ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CliRemoteConfig {
    pub url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default = "default_true")]
    pub browser: bool,
    #[serde(default)]
    pub remote: CliRemoteConfig,
    /// Per-action hook definitions for the CLI runtime.
    /// Flattened so `[cli.task_add.hooks.foo]` binds directly.
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    /// Contract-aggregate hook definitions for the CLI runtime.
    /// Flattened so `[cli.contract_add.hooks.foo]` binds directly.
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    /// Project-aggregate hook definitions for the CLI runtime.
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    /// User-aggregate hook definitions for the CLI runtime.
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    /// Metadata-field-aggregate hook definitions for the CLI runtime.
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            browser: true,
            remote: CliRemoteConfig::default(),
            hooks: TaskActionHooks::default(),
            contract_hooks: ContractActionHooks::default(),
            project_hooks: ProjectActionHooks::default(),
            user_hooks: UserActionHooks::default(),
            metadata_field_hooks: MetadataFieldActionHooks::default(),
        }
    }
}

// --- Server config ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerRelayConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    /// Per-action hook definitions that fire when this binary runs as relay.
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    /// Contract-aggregate hook definitions for the relay runtime.
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    /// Project-aggregate hook definitions for the relay runtime.
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    /// User-aggregate hook definitions for the relay runtime.
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    /// Metadata-field-aggregate hook definitions for the relay runtime.
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerRemoteConfig {
    /// Per-action hook definitions that fire when this binary runs as the
    /// direct (non-relay) server.
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    /// Contract-aggregate hook definitions for the direct-server runtime.
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    /// Project-aggregate hook definitions for the direct-server runtime.
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    /// User-aggregate hook definitions for the direct-server runtime.
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    /// Metadata-field-aggregate hook definitions for the direct-server runtime.
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub relay: ServerRelayConfig,
    #[serde(default)]
    pub remote: ServerRemoteConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

// --- Auth types (unchanged) ---

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct OidcConfig {
    pub issuer_url: Option<String>,
    pub client_id: Option<String>,
    pub username_claim: Option<String>,
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub required_claims: HashMap<String, String>,
    #[serde(default)]
    pub callback_ports: Vec<String>,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default = "default_oidc_groups_claim")]
    pub groups_claim: String,
    pub master_group: Option<String>,
}

impl OidcConfig {
    /// Returns true when both issuer_url and client_id are set,
    /// meaning OIDC JWT verification should be enabled.
    pub fn is_configured(&self) -> bool {
        self.issuer_url.is_some() && self.client_id.is_some()
    }
}

fn default_oidc_scopes() -> Vec<String> {
    vec!["openid".to_string(), "profile".to_string()]
}

fn default_oidc_groups_claim() -> String {
    "groups".to_string()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SessionConfig {
    pub ttl: Option<String>,
    pub inactive_ttl: Option<String>,
    pub max_per_user: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ApiKeyConfig {
    pub master_key: Option<String>,
    pub master_key_arn: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct TrustedHeadersConfig {
    pub subject_header: Option<String>,
    pub name_header: Option<String>,
    pub display_name_header: Option<String>,
    pub email_header: Option<String>,
    pub groups_header: Option<String>,
    pub scope_header: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub master_group: Option<String>,
}

impl TrustedHeadersConfig {
    pub fn is_configured(&self) -> bool {
        self.subject_header.is_some()
    }
}

/// DEVELOPMENT-ONLY auth bypass. Refuses to start with SENKO_ENV=production.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DevBypassConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(default)]
    pub api_key: ApiKeyConfig,
    #[serde(default)]
    pub oidc: OidcConfig,
    #[serde(default)]
    pub trusted_headers: TrustedHeadersConfig,
    #[serde(default)]
    pub dev_bypass: DevBypassConfig,
}

impl AuthConfig {
    /// Returns true when at least one authentication method is configured.
    pub fn is_configured(&self) -> bool {
        self.oidc.is_configured()
            || self.api_key.master_key.is_some()
            || self.trusted_headers.is_configured()
            || self.dev_bypass.enabled
    }

    /// Returns an error message if more than one authentication mode is configured.
    pub fn validate_exclusive(&self) -> Result<(), String> {
        let count = [
            self.oidc.is_configured(),
            self.api_key.master_key.is_some(),
            self.trusted_headers.is_configured(),
            self.dev_bypass.enabled,
        ]
        .iter()
        .filter(|&&v| v)
        .count();

        if count > 1 {
            Err("only one authentication mode may be configured at a time \
                 (api_key, oidc, trusted_headers, or dev_bypass)"
                .to_string())
        } else {
            Ok(())
        }
    }
}

// --- Project / User ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserConfig {
    pub name: Option<String>,
}

// --- Log config ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogConfig {
    pub dir: Option<String>,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub format: LogFormat,
    #[serde(default)]
    pub hook_output: HookOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookOutput {
    #[default]
    File,
    Stdout,
    Both,
}

fn default_log_level() -> String {
    "info".to_string()
}

// --- Backend config ---

#[cfg(feature = "postgres")]
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PostgresConfig {
    pub url: Option<String>,
    pub url_arn: Option<String>,
    pub rds_secrets_arn: Option<String>,
    pub sslrootcert: Option<String>,
    pub max_connections: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SqliteConfig {
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    #[serde(default)]
    pub sqlite: SqliteConfig,
    #[cfg(feature = "postgres")]
    #[serde(default)]
    pub postgres: Option<PostgresConfig>,
}

// --- Workflow config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub merge_via: MergeVia,
    #[serde(default = "default_true")]
    pub auto_merge: bool,
    #[serde(default)]
    pub branch_mode: BranchMode,
    #[serde(default)]
    pub merge_strategy: MergeStrategy,
    #[serde(default)]
    pub branch_template: Option<String>,
    /// All stages (built-in presets + user extensions) live here.
    #[serde(default, flatten)]
    pub stages: HashMap<String, WorkflowStageConfig>,
}

fn default_true() -> bool {
    true
}

/// Default `HookDef.timeout_secs` value: 30 seconds.
///
/// Picked so configs that pre-date the field continue to work, while
/// guaranteeing every hook has a bounded lifetime (Contract #8 D1 added
/// the timeout machinery; before it `run_hook_command` waited forever).
/// Hooks that need a longer ceiling can set `timeout_secs` explicitly.
fn default_hook_timeout_secs() -> u64 {
    30
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            merge_via: MergeVia::default(),
            auto_merge: true,
            branch_mode: BranchMode::default(),
            merge_strategy: MergeStrategy::default(),
            branch_template: None,
            stages: HashMap::new(),
        }
    }
}

impl WorkflowConfig {
    pub fn stage(&self, name: &str) -> Option<&WorkflowStageConfig> {
        self.stages.get(name)
    }

    pub fn stage_or_default(&self, name: &str) -> WorkflowStageConfig {
        self.stages.get(name).cloned().unwrap_or_default()
    }
}

// --- RawConfig for layered merging ---

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawConfig {
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
    pub server: RawServerConfig,
    #[serde(default)]
    pub cli: RawCliConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawWorkflowConfig {
    #[serde(alias = "completion_mode")]
    pub merge_via: Option<MergeVia>,
    pub auto_merge: Option<bool>,
    pub branch_mode: Option<BranchMode>,
    pub merge_strategy: Option<MergeStrategy>,
    pub branch_template: Option<String>,
    /// All remaining keys under `[workflow]` are treated as stages.
    #[serde(flatten)]
    pub stages: HashMap<String, WorkflowStageConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawBackendConfig {
    #[serde(default)]
    pub sqlite: RawSqliteConfig,
    #[cfg(feature = "postgres")]
    pub postgres: Option<PostgresConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawSqliteConfig {
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawLogConfig {
    pub dir: Option<String>,
    pub level: Option<String>,
    pub format: Option<LogFormat>,
    pub hook_output: Option<HookOutput>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawServerRelayConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawServerRemoteConfig {
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawServerConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub relay: RawServerRelayConfig,
    #[serde(default)]
    pub remote: RawServerRemoteConfig,
    #[serde(default)]
    pub auth: RawAuthConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawCliConfig {
    pub browser: Option<bool>,
    #[serde(default)]
    pub remote: RawCliRemoteConfig,
    #[serde(default, flatten)]
    pub hooks: TaskActionHooks,
    #[serde(default, flatten)]
    pub contract_hooks: ContractActionHooks,
    #[serde(default, flatten)]
    pub project_hooks: ProjectActionHooks,
    #[serde(default, flatten)]
    pub user_hooks: UserActionHooks,
    #[serde(default, flatten)]
    pub metadata_field_hooks: MetadataFieldActionHooks,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawCliRemoteConfig {
    pub url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawOidcConfig {
    pub issuer_url: Option<String>,
    pub client_id: Option<String>,
    pub username_claim: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub required_claims: Option<HashMap<String, String>>,
    pub callback_ports: Option<Vec<String>>,
    #[serde(default)]
    pub session: RawSessionConfig,
    pub groups_claim: Option<String>,
    pub master_group: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawSessionConfig {
    pub ttl: Option<String>,
    pub inactive_ttl: Option<String>,
    pub max_per_user: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawApiKeyConfig {
    pub master_key: Option<String>,
    pub master_key_arn: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawTrustedHeadersConfig {
    pub subject_header: Option<String>,
    pub name_header: Option<String>,
    pub display_name_header: Option<String>,
    pub email_header: Option<String>,
    pub groups_header: Option<String>,
    pub scope_header: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub master_group: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawDevBypassConfig {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawAuthConfig {
    #[serde(default)]
    pub api_key: RawApiKeyConfig,
    #[serde(default)]
    pub oidc: RawOidcConfig,
    #[serde(default)]
    pub trusted_headers: RawTrustedHeadersConfig,
    #[serde(default)]
    pub dev_bypass: RawDevBypassConfig,
}

impl RawConfig {
    /// Merge two configs: `self` is the base (lower priority), `overlay` wins.
    pub fn merge(self, overlay: RawConfig) -> RawConfig {
        RawConfig {
            workflow: RawWorkflowConfig {
                merge_via: overlay.workflow.merge_via.or(self.workflow.merge_via),
                auto_merge: overlay.workflow.auto_merge.or(self.workflow.auto_merge),
                branch_mode: overlay.workflow.branch_mode.or(self.workflow.branch_mode),
                merge_strategy: overlay
                    .workflow
                    .merge_strategy
                    .or(self.workflow.merge_strategy),
                branch_template: overlay
                    .workflow
                    .branch_template
                    .or(self.workflow.branch_template),
                stages: merge_stages(self.workflow.stages, overlay.workflow.stages),
            },
            backend: RawBackendConfig {
                sqlite: RawSqliteConfig {
                    db_path: overlay
                        .backend
                        .sqlite
                        .db_path
                        .or(self.backend.sqlite.db_path),
                },
                #[cfg(feature = "postgres")]
                postgres: overlay.backend.postgres.or(self.backend.postgres),
            },
            log: RawLogConfig {
                dir: overlay.log.dir.or(self.log.dir),
                level: overlay.log.level.or(self.log.level),
                format: overlay.log.format.or(self.log.format),
                hook_output: overlay.log.hook_output.or(self.log.hook_output),
            },
            project: ProjectConfig {
                name: overlay.project.name.or(self.project.name),
            },
            user: UserConfig {
                name: overlay.user.name.or(self.user.name),
            },
            server: RawServerConfig {
                host: overlay.server.host.or(self.server.host),
                port: overlay.server.port.or(self.server.port),
                relay: RawServerRelayConfig {
                    url: overlay.server.relay.url.or(self.server.relay.url),
                    token: overlay.server.relay.token.or(self.server.relay.token),
                    hooks: merge_task_action_hooks(
                        self.server.relay.hooks,
                        overlay.server.relay.hooks,
                    ),
                    contract_hooks: merge_contract_action_hooks(
                        self.server.relay.contract_hooks,
                        overlay.server.relay.contract_hooks,
                    ),
                    project_hooks: merge_project_action_hooks(
                        self.server.relay.project_hooks,
                        overlay.server.relay.project_hooks,
                    ),
                    user_hooks: merge_user_action_hooks(
                        self.server.relay.user_hooks,
                        overlay.server.relay.user_hooks,
                    ),
                    metadata_field_hooks: merge_metadata_field_action_hooks(
                        self.server.relay.metadata_field_hooks,
                        overlay.server.relay.metadata_field_hooks,
                    ),
                },
                remote: RawServerRemoteConfig {
                    hooks: merge_task_action_hooks(
                        self.server.remote.hooks,
                        overlay.server.remote.hooks,
                    ),
                    contract_hooks: merge_contract_action_hooks(
                        self.server.remote.contract_hooks,
                        overlay.server.remote.contract_hooks,
                    ),
                    project_hooks: merge_project_action_hooks(
                        self.server.remote.project_hooks,
                        overlay.server.remote.project_hooks,
                    ),
                    user_hooks: merge_user_action_hooks(
                        self.server.remote.user_hooks,
                        overlay.server.remote.user_hooks,
                    ),
                    metadata_field_hooks: merge_metadata_field_action_hooks(
                        self.server.remote.metadata_field_hooks,
                        overlay.server.remote.metadata_field_hooks,
                    ),
                },
                auth: RawAuthConfig {
                    api_key: RawApiKeyConfig {
                        master_key: overlay.server.auth.api_key.master_key.or(self
                            .server
                            .auth
                            .api_key
                            .master_key),
                        master_key_arn: overlay.server.auth.api_key.master_key_arn.or(self
                            .server
                            .auth
                            .api_key
                            .master_key_arn),
                    },
                    trusted_headers: RawTrustedHeadersConfig {
                        subject_header: overlay.server.auth.trusted_headers.subject_header.or(self
                            .server
                            .auth
                            .trusted_headers
                            .subject_header),
                        name_header: overlay.server.auth.trusted_headers.name_header.or(self
                            .server
                            .auth
                            .trusted_headers
                            .name_header),
                        display_name_header: overlay
                            .server
                            .auth
                            .trusted_headers
                            .display_name_header
                            .or(self.server.auth.trusted_headers.display_name_header),
                        email_header: overlay.server.auth.trusted_headers.email_header.or(self
                            .server
                            .auth
                            .trusted_headers
                            .email_header),
                        groups_header: overlay.server.auth.trusted_headers.groups_header.or(self
                            .server
                            .auth
                            .trusted_headers
                            .groups_header),
                        scope_header: overlay.server.auth.trusted_headers.scope_header.or(self
                            .server
                            .auth
                            .trusted_headers
                            .scope_header),
                        oidc_issuer_url: overlay
                            .server
                            .auth
                            .trusted_headers
                            .oidc_issuer_url
                            .or(self.server.auth.trusted_headers.oidc_issuer_url),
                        oidc_client_id: overlay.server.auth.trusted_headers.oidc_client_id.or(self
                            .server
                            .auth
                            .trusted_headers
                            .oidc_client_id),
                        master_group: overlay.server.auth.trusted_headers.master_group.or(self
                            .server
                            .auth
                            .trusted_headers
                            .master_group),
                    },
                    oidc: RawOidcConfig {
                        issuer_url: overlay.server.auth.oidc.issuer_url.or(self
                            .server
                            .auth
                            .oidc
                            .issuer_url),
                        client_id: overlay.server.auth.oidc.client_id.or(self
                            .server
                            .auth
                            .oidc
                            .client_id),
                        username_claim: overlay.server.auth.oidc.username_claim.or(self
                            .server
                            .auth
                            .oidc
                            .username_claim),
                        scopes: overlay
                            .server
                            .auth
                            .oidc
                            .scopes
                            .or(self.server.auth.oidc.scopes),
                        required_claims: overlay.server.auth.oidc.required_claims.or(self
                            .server
                            .auth
                            .oidc
                            .required_claims),
                        callback_ports: overlay.server.auth.oidc.callback_ports.or(self
                            .server
                            .auth
                            .oidc
                            .callback_ports),
                        session: RawSessionConfig {
                            ttl: overlay.server.auth.oidc.session.ttl.or(self
                                .server
                                .auth
                                .oidc
                                .session
                                .ttl),
                            inactive_ttl: overlay.server.auth.oidc.session.inactive_ttl.or(self
                                .server
                                .auth
                                .oidc
                                .session
                                .inactive_ttl),
                            max_per_user: overlay.server.auth.oidc.session.max_per_user.or(self
                                .server
                                .auth
                                .oidc
                                .session
                                .max_per_user),
                        },
                        groups_claim: overlay.server.auth.oidc.groups_claim.or(self
                            .server
                            .auth
                            .oidc
                            .groups_claim),
                        master_group: overlay.server.auth.oidc.master_group.or(self
                            .server
                            .auth
                            .oidc
                            .master_group),
                    },
                    dev_bypass: RawDevBypassConfig {
                        enabled: overlay.server.auth.dev_bypass.enabled.or(self
                            .server
                            .auth
                            .dev_bypass
                            .enabled),
                    },
                },
            },
            cli: RawCliConfig {
                browser: overlay.cli.browser.or(self.cli.browser),
                remote: RawCliRemoteConfig {
                    url: overlay.cli.remote.url.or(self.cli.remote.url),
                    token: overlay.cli.remote.token.or(self.cli.remote.token),
                },
                hooks: merge_task_action_hooks(self.cli.hooks, overlay.cli.hooks),
                contract_hooks: merge_contract_action_hooks(
                    self.cli.contract_hooks,
                    overlay.cli.contract_hooks,
                ),
                project_hooks: merge_project_action_hooks(
                    self.cli.project_hooks,
                    overlay.cli.project_hooks,
                ),
                user_hooks: merge_user_action_hooks(self.cli.user_hooks, overlay.cli.user_hooks),
                metadata_field_hooks: merge_metadata_field_action_hooks(
                    self.cli.metadata_field_hooks,
                    overlay.cli.metadata_field_hooks,
                ),
            },
        }
    }

    /// Resolve to final Config, filling None values with defaults.
    pub fn resolve(self) -> Config {
        Config {
            workflow: WorkflowConfig {
                merge_via: self.workflow.merge_via.unwrap_or_default(),
                auto_merge: self.workflow.auto_merge.unwrap_or(true),
                branch_mode: self.workflow.branch_mode.unwrap_or_default(),
                merge_strategy: self.workflow.merge_strategy.unwrap_or_default(),
                branch_template: self.workflow.branch_template,
                stages: self.workflow.stages,
            },
            backend: BackendConfig {
                sqlite: SqliteConfig {
                    db_path: self.backend.sqlite.db_path,
                },
                #[cfg(feature = "postgres")]
                postgres: self.backend.postgres,
            },
            log: LogConfig {
                dir: self.log.dir,
                level: self.log.level.unwrap_or_else(default_log_level),
                format: self.log.format.unwrap_or_default(),
                hook_output: self.log.hook_output.unwrap_or_default(),
            },
            project: self.project,
            user: self.user,
            server: ServerConfig {
                host: self.server.host,
                port: self.server.port,
                relay: ServerRelayConfig {
                    url: self.server.relay.url,
                    token: self.server.relay.token,
                    hooks: self.server.relay.hooks,
                    contract_hooks: self.server.relay.contract_hooks,
                    project_hooks: self.server.relay.project_hooks,
                    user_hooks: self.server.relay.user_hooks,
                    metadata_field_hooks: self.server.relay.metadata_field_hooks,
                },
                remote: ServerRemoteConfig {
                    hooks: self.server.remote.hooks,
                    contract_hooks: self.server.remote.contract_hooks,
                    project_hooks: self.server.remote.project_hooks,
                    user_hooks: self.server.remote.user_hooks,
                    metadata_field_hooks: self.server.remote.metadata_field_hooks,
                },
                auth: AuthConfig {
                    api_key: ApiKeyConfig {
                        master_key: self.server.auth.api_key.master_key,
                        master_key_arn: self.server.auth.api_key.master_key_arn,
                    },
                    oidc: OidcConfig {
                        issuer_url: self.server.auth.oidc.issuer_url,
                        client_id: self.server.auth.oidc.client_id,
                        username_claim: self.server.auth.oidc.username_claim,
                        scopes: self
                            .server
                            .auth
                            .oidc
                            .scopes
                            .unwrap_or_else(default_oidc_scopes),
                        required_claims: self.server.auth.oidc.required_claims.unwrap_or_default(),
                        callback_ports: self.server.auth.oidc.callback_ports.unwrap_or_default(),
                        session: SessionConfig {
                            ttl: self.server.auth.oidc.session.ttl,
                            inactive_ttl: self.server.auth.oidc.session.inactive_ttl,
                            max_per_user: self.server.auth.oidc.session.max_per_user,
                        },
                        groups_claim: self
                            .server
                            .auth
                            .oidc
                            .groups_claim
                            .unwrap_or_else(default_oidc_groups_claim),
                        master_group: self.server.auth.oidc.master_group,
                    },
                    trusted_headers: TrustedHeadersConfig {
                        subject_header: self.server.auth.trusted_headers.subject_header,
                        name_header: self.server.auth.trusted_headers.name_header,
                        display_name_header: self.server.auth.trusted_headers.display_name_header,
                        email_header: self.server.auth.trusted_headers.email_header,
                        groups_header: self.server.auth.trusted_headers.groups_header,
                        scope_header: self.server.auth.trusted_headers.scope_header,
                        oidc_issuer_url: self.server.auth.trusted_headers.oidc_issuer_url,
                        oidc_client_id: self.server.auth.trusted_headers.oidc_client_id,
                        master_group: self.server.auth.trusted_headers.master_group,
                    },
                    dev_bypass: DevBypassConfig {
                        enabled: self.server.auth.dev_bypass.enabled.unwrap_or(false),
                    },
                },
            },
            cli: CliConfig {
                browser: self.cli.browser.unwrap_or(true),
                remote: CliRemoteConfig {
                    url: self.cli.remote.url,
                    token: self.cli.remote.token,
                },
                hooks: self.cli.hooks,
                contract_hooks: self.cli.contract_hooks,
                project_hooks: self.cli.project_hooks,
                user_hooks: self.cli.user_hooks,
                metadata_field_hooks: self.cli.metadata_field_hooks,
            },
            xdg: XdgDirs::default(),
        }
    }
}

/// Merge workflow stages: overlay stages take precedence over base. For
/// stages present in both, the overlay stage replaces the base stage wholesale
/// (we do not attempt deep merge inside a stage because `extra` fields have
/// open-ended shape).
fn merge_stages(
    mut base: HashMap<String, WorkflowStageConfig>,
    overlay: HashMap<String, WorkflowStageConfig>,
) -> HashMap<String, WorkflowStageConfig> {
    for (name, stage) in overlay {
        base.insert(name, stage);
    }
    base
}

/// Merge per-action hook maps. For a given action, overlay hook names override
/// base hook names; hook names unique to either side are preserved.
fn merge_action_config(base: ActionConfig, overlay: ActionConfig) -> ActionConfig {
    let mut hooks = base.hooks;
    for (name, def) in overlay.hooks {
        hooks.insert(name, def);
    }
    ActionConfig { hooks }
}

fn merge_task_action_hooks(base: TaskActionHooks, overlay: TaskActionHooks) -> TaskActionHooks {
    TaskActionHooks {
        task_add: merge_action_config(base.task_add, overlay.task_add),
        task_update: merge_action_config(base.task_update, overlay.task_update),
        task_publish: merge_action_config(base.task_publish, overlay.task_publish),
        task_start: merge_action_config(base.task_start, overlay.task_start),
        task_resume: merge_action_config(base.task_resume, overlay.task_resume),
        task_complete: merge_action_config(base.task_complete, overlay.task_complete),
        task_cancel: merge_action_config(base.task_cancel, overlay.task_cancel),
        task_select: merge_action_config(base.task_select, overlay.task_select),
    }
}

fn merge_contract_action_hooks(
    base: ContractActionHooks,
    overlay: ContractActionHooks,
) -> ContractActionHooks {
    ContractActionHooks {
        contract_add: merge_action_config(base.contract_add, overlay.contract_add),
        contract_edit: merge_action_config(base.contract_edit, overlay.contract_edit),
        contract_delete: merge_action_config(base.contract_delete, overlay.contract_delete),
        contract_dod_check: merge_action_config(
            base.contract_dod_check,
            overlay.contract_dod_check,
        ),
        contract_dod_uncheck: merge_action_config(
            base.contract_dod_uncheck,
            overlay.contract_dod_uncheck,
        ),
        contract_note_add: merge_action_config(base.contract_note_add, overlay.contract_note_add),
    }
}

fn merge_project_action_hooks(
    base: ProjectActionHooks,
    overlay: ProjectActionHooks,
) -> ProjectActionHooks {
    ProjectActionHooks {
        project_create: merge_action_config(base.project_create, overlay.project_create),
        project_update: merge_action_config(base.project_update, overlay.project_update),
        project_member_add: merge_action_config(
            base.project_member_add,
            overlay.project_member_add,
        ),
        project_member_remove: merge_action_config(
            base.project_member_remove,
            overlay.project_member_remove,
        ),
        project_member_role_change: merge_action_config(
            base.project_member_role_change,
            overlay.project_member_role_change,
        ),
    }
}

fn merge_user_action_hooks(base: UserActionHooks, overlay: UserActionHooks) -> UserActionHooks {
    UserActionHooks {
        user_create: merge_action_config(base.user_create, overlay.user_create),
        user_update: merge_action_config(base.user_update, overlay.user_update),
        user_api_key_issue: merge_action_config(
            base.user_api_key_issue,
            overlay.user_api_key_issue,
        ),
        user_api_key_revoke: merge_action_config(
            base.user_api_key_revoke,
            overlay.user_api_key_revoke,
        ),
        user_session_revoke: merge_action_config(
            base.user_session_revoke,
            overlay.user_session_revoke,
        ),
    }
}

fn merge_metadata_field_action_hooks(
    base: MetadataFieldActionHooks,
    overlay: MetadataFieldActionHooks,
) -> MetadataFieldActionHooks {
    MetadataFieldActionHooks {
        metadata_field_define: merge_action_config(
            base.metadata_field_define,
            overlay.metadata_field_define,
        ),
        metadata_field_remove: merge_action_config(
            base.metadata_field_remove,
            overlay.metadata_field_remove,
        ),
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
    pub server_port: Option<u16>,
    pub server_host: Option<String>,
    /// DEVELOPMENT-ONLY: enable auth bypass via `--dev-no-auth`. Cannot be reset by lower-priority sources.
    pub dev_no_auth: bool,
    /// Force the local backend: clears `cli.remote.url` so backend selection
    /// falls through to sqlite/postgres even when a remote URL is configured.
    pub local: bool,
}

impl Config {
    /// Apply environment variable overrides. Call after `RawConfig::resolve()`.
    /// Priority: env > config.toml defaults.
    pub fn apply_env(&mut self) {
        // Workflow settings
        // Check SENKO_MERGE_VIA first, then fallback to deprecated SENKO_COMPLETION_MODE
        let merge_via_env = std::env::var("SENKO_MERGE_VIA").ok().or_else(|| {
            std::env::var("SENKO_COMPLETION_MODE").ok().inspect(|_| {
                eprintln!("warning: SENKO_COMPLETION_MODE is deprecated, use SENKO_MERGE_VIA");
            })
        });
        if let Some(val) = merge_via_env {
            match val.as_str() {
                "direct" => self.workflow.merge_via = MergeVia::Direct,
                "pr" => self.workflow.merge_via = MergeVia::Pr,
                "merge_then_complete" => {
                    eprintln!(
                        "warning: merge_via value \"merge_then_complete\" is deprecated, use \"direct\""
                    );
                    self.workflow.merge_via = MergeVia::Direct;
                }
                "pr_then_complete" => {
                    eprintln!(
                        "warning: merge_via value \"pr_then_complete\" is deprecated, use \"pr\""
                    );
                    self.workflow.merge_via = MergeVia::Pr;
                }
                other => eprintln!("warning: unknown SENKO_MERGE_VIA={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_AUTO_MERGE") {
            match val.to_lowercase().as_str() {
                "true" | "1" | "yes" => self.workflow.auto_merge = true,
                "false" | "0" | "no" => self.workflow.auto_merge = false,
                other => eprintln!("warning: unknown SENKO_AUTO_MERGE={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_BRANCH_MODE_TYPE") {
            match val.as_str() {
                "worktree" => self.workflow.branch_mode.kind = BranchModeType::Worktree,
                "branch" => self.workflow.branch_mode.kind = BranchModeType::Branch,
                other => eprintln!("warning: unknown SENKO_BRANCH_MODE_TYPE={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_BRANCH_MODE_CREATE") {
            match val.to_lowercase().as_str() {
                "true" | "1" | "yes" => self.workflow.branch_mode.create = true,
                "false" | "0" | "no" => self.workflow.branch_mode.create = false,
                other => eprintln!("warning: unknown SENKO_BRANCH_MODE_CREATE={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_MERGE_STRATEGY") {
            match val.as_str() {
                "rebase" => self.workflow.merge_strategy = MergeStrategy::Rebase,
                "squash" => self.workflow.merge_strategy = MergeStrategy::Squash,
                other => eprintln!("warning: unknown SENKO_MERGE_STRATEGY={other}, ignoring"),
            }
        }

        // Server relay settings
        if let Ok(val) = std::env::var("SENKO_SERVER_RELAY_URL")
            && !val.is_empty()
        {
            self.server.relay.url = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_SERVER_RELAY_TOKEN")
            && !val.is_empty()
        {
            self.server.relay.token = Some(val);
        }

        // CLI remote settings
        if let Ok(val) = std::env::var("SENKO_CLI_REMOTE_URL")
            && !val.is_empty()
        {
            self.cli.remote.url = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_CLI_REMOTE_TOKEN")
            && !val.is_empty()
        {
            self.cli.remote.token = Some(val);
        }

        // Server auth settings
        if let Ok(val) = std::env::var("SENKO_AUTH_API_KEY_MASTER_KEY")
            && !val.is_empty()
        {
            self.server.auth.api_key.master_key = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_API_KEY_MASTER_KEY_ARN")
            && !val.is_empty()
        {
            self.server.auth.api_key.master_key_arn = Some(val);
        }

        // Server OIDC settings
        if let Ok(val) = std::env::var("SENKO_OIDC_ISSUER_URL")
            && !val.is_empty()
        {
            self.server.auth.oidc.issuer_url = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_OIDC_CLIENT_ID")
            && !val.is_empty()
        {
            self.server.auth.oidc.client_id = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_OIDC_USERNAME_CLAIM")
            && !val.is_empty()
        {
            self.server.auth.oidc.username_claim = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_OIDC_CALLBACK_PORTS")
            && !val.is_empty()
        {
            self.server.auth.oidc.callback_ports = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        // Server trusted headers settings
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_SUBJECT_HEADER")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.subject_header = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_NAME_HEADER")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.name_header = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_EMAIL_HEADER")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.email_header = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_GROUPS_HEADER")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.groups_header = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_SCOPE_HEADER")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.scope_header = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_OIDC_ISSUER_URL")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.oidc_issuer_url = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_OIDC_CLIENT_ID")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.oidc_client_id = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_TRUSTED_HEADERS_MASTER_GROUP")
            && !val.is_empty()
        {
            self.server.auth.trusted_headers.master_group = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_OIDC_GROUPS_CLAIM")
            && !val.is_empty()
        {
            self.server.auth.oidc.groups_claim = val;
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_OIDC_MASTER_GROUP")
            && !val.is_empty()
        {
            self.server.auth.oidc.master_group = Some(val);
        }

        // Server OIDC session settings
        if let Ok(val) = std::env::var("SENKO_AUTH_OIDC_SESSION_TTL")
            && !val.is_empty()
        {
            self.server.auth.oidc.session.ttl = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_OIDC_SESSION_INACTIVE_TTL")
            && !val.is_empty()
        {
            self.server.auth.oidc.session.inactive_ttl = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_AUTH_OIDC_SESSION_MAX_PER_USER")
            && !val.is_empty()
            && let Ok(n) = val.parse::<u32>()
        {
            self.server.auth.oidc.session.max_per_user = Some(n);
        }

        // Server host/port
        if let Ok(val) = std::env::var("SENKO_SERVER_HOST")
            && !val.is_empty()
        {
            self.server.host = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_SERVER_PORT")
            && let Ok(port) = val.parse::<u16>()
        {
            self.server.port = Some(port);
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
            if let Ok(val) = std::env::var("SENKO_POSTGRES_URL_ARN") {
                if !val.is_empty() {
                    self.backend
                        .postgres
                        .get_or_insert_with(PostgresConfig::default)
                        .url_arn = Some(val);
                }
            }
            if let Ok(val) = std::env::var("SENKO_POSTGRES_RDS_SECRETS_ARN") {
                if !val.is_empty() {
                    self.backend
                        .postgres
                        .get_or_insert_with(PostgresConfig::default)
                        .rds_secrets_arn = Some(val);
                }
            }
            if let Ok(val) = std::env::var("SENKO_POSTGRES_SSLROOTCERT") {
                if !val.is_empty() {
                    self.backend
                        .postgres
                        .get_or_insert_with(PostgresConfig::default)
                        .sslrootcert = Some(val);
                }
            }
            if let Ok(val) = std::env::var("SENKO_POSTGRES_MAX_CONNECTIONS") {
                if !val.is_empty() {
                    if let Ok(n) = val.parse::<u32>() {
                        self.backend
                            .postgres
                            .get_or_insert_with(PostgresConfig::default)
                            .max_connections = Some(n);
                    }
                }
            }
        }

        // User settings
        if let Ok(val) = std::env::var("SENKO_USER")
            && !val.is_empty()
        {
            self.user.name = Some(val);
        }

        // Project settings
        if let Ok(val) = std::env::var("SENKO_PROJECT")
            && !val.is_empty()
        {
            self.project.name = Some(val);
        }

        // Backend SQLite settings
        if let Ok(val) = std::env::var("SENKO_DB_PATH")
            && !val.is_empty()
        {
            self.backend.sqlite.db_path = Some(val);
        }

        // Log settings
        if let Ok(val) = std::env::var("SENKO_LOG_DIR")
            && !val.is_empty()
        {
            self.log.dir = Some(val);
        }
        if let Ok(val) = std::env::var("SENKO_LOG_LEVEL")
            && !val.is_empty()
        {
            self.log.level = val;
        }
        if let Ok(val) = std::env::var("SENKO_LOG_FORMAT") {
            match val.to_lowercase().as_str() {
                "json" => self.log.format = LogFormat::Json,
                "pretty" => self.log.format = LogFormat::Pretty,
                other => eprintln!("warning: unknown SENKO_LOG_FORMAT={other}, ignoring"),
            }
        }
        if let Ok(val) = std::env::var("SENKO_LOG_HOOK_OUTPUT") {
            match val.to_lowercase().as_str() {
                "file" => self.log.hook_output = HookOutput::File,
                "stdout" => self.log.hook_output = HookOutput::Stdout,
                "both" => self.log.hook_output = HookOutput::Both,
                other => eprintln!("warning: unknown SENKO_LOG_HOOK_OUTPUT={other}, ignoring"),
            }
        }

        // Server settings
        if let Ok(val) = std::env::var("SENKO_PORT")
            && let Ok(port) = val.parse::<u16>()
        {
            self.server.port = Some(port);
        }
        if let Ok(val) = std::env::var("SENKO_HOST")
            && !val.is_empty()
        {
            self.server.host = Some(val);
        }
    }

    /// Apply CLI argument overrides. Call after `apply_env()`.
    /// Priority: CLI > env > config.toml > defaults.
    pub fn apply_cli(&mut self, overrides: &CliOverrides) {
        if let Some(ref dir) = overrides.log_dir {
            self.log.dir = Some(dir.clone());
        }
        if let Some(ref path) = overrides.db_path {
            self.backend.sqlite.db_path = Some(path.clone());
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
        if let Some(port) = overrides.server_port {
            self.server.port = Some(port);
        }
        if let Some(ref host) = overrides.server_host {
            self.server.host = Some(host.clone());
        }
        if overrides.dev_no_auth {
            self.server.auth.dev_bypass.enabled = true;
        }
        if overrides.local {
            self.cli.remote.url = None;
            self.cli.remote.token = None;
        }
    }

    pub fn server_port_or(&self, default: u16) -> u16 {
        self.server.port.unwrap_or(default)
    }

    pub fn server_port_is_explicit(&self) -> bool {
        self.server.port.is_some()
    }

    pub fn effective_server_host(&self) -> String {
        self.server
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }

    /// Resolve secrets from AWS Secrets Manager using ARN config fields.
    /// Call after `apply_env()`. ARN-resolved values overwrite direct values.
    #[cfg(feature = "aws-secrets")]
    pub async fn resolve_secrets(&mut self) -> anyhow::Result<()> {
        use crate::infra::secrets::SecretsManagerClient;

        let client = SecretsManagerClient::new(None);
        self.resolve_secrets_with(&client).await
    }

    /// Resolve secrets using the provided client. Separated for testability.
    #[cfg(feature = "aws-secrets")]
    pub(crate) async fn resolve_secrets_with(
        &mut self,
        client: &crate::infra::secrets::SecretsManagerClient,
    ) -> anyhow::Result<()> {
        use anyhow::Context;

        if let Some(ref arn) = self.server.auth.api_key.master_key_arn {
            let secret = client.get_secret(arn).await?;
            self.server.auth.api_key.master_key = Some(secret);
        }

        #[cfg(feature = "postgres")]
        {
            let pg = self.backend.postgres.as_ref();
            let rds_arn = pg.and_then(|p| p.rds_secrets_arn.clone());
            let url_arn = pg.and_then(|p| p.url_arn.clone());
            let sslrootcert = pg.and_then(|p| p.sslrootcert.clone());

            if let Some(ref arn) = rds_arn {
                let secret = client.get_secret(arn).await?;
                let url = build_rds_url(&secret, sslrootcert.as_deref())
                    .with_context(|| format!("failed to parse RDS JSON secret from {arn}"))?;
                self.backend
                    .postgres
                    .get_or_insert_with(PostgresConfig::default)
                    .url = Some(url);
            } else if let Some(ref arn) = url_arn {
                let secret = client.get_secret(arn).await?;
                self.backend
                    .postgres
                    .get_or_insert_with(PostgresConfig::default)
                    .url = Some(secret);
            }
        }

        Ok(())
    }
}

#[cfg(all(feature = "postgres", feature = "aws-secrets"))]
fn build_rds_url(json_str: &str, sslrootcert: Option<&str>) -> anyhow::Result<String> {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

    #[derive(serde::Deserialize)]
    struct RdsSecret {
        username: String,
        password: String,
        host: String,
        port: Option<u16>,
        dbname: Option<String>,
    }

    let secret: RdsSecret = serde_json::from_str(json_str)?;
    let port = secret.port.unwrap_or(5432);
    let dbname = secret.dbname.as_deref().unwrap_or("postgres");
    let encoded_user = utf8_percent_encode(&secret.username, NON_ALPHANUMERIC);
    let encoded_pass = utf8_percent_encode(&secret.password, NON_ALPHANUMERIC);

    let mut url = format!(
        "postgres://{encoded_user}:{encoded_pass}@{}:{port}/{dbname}",
        secret.host
    );

    if let Some(cert_path) = sslrootcert {
        url.push_str("?sslmode=verify-full&sslrootcert=");
        url.push_str(cert_path);
    }

    Ok(url)
}

// Deserializer helpers for backward-compat-free strictness
#[allow(dead_code)]
fn err_deny_unknown<'de, D: Deserializer<'de>>(_d: D) -> Result<(), D::Error> {
    Ok(())
}

#[cfg(all(test, feature = "aws-secrets"))]
mod resolve_secrets_tests {
    use super::*;
    use crate::infra::secrets::{SecretFetcher, SecretsManagerClient};
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct FakeSecretFetcher {
        secrets: HashMap<String, String>,
    }

    #[async_trait]
    impl SecretFetcher for FakeSecretFetcher {
        async fn fetch_secret(&self, arn: &str) -> anyhow::Result<String> {
            self.secrets
                .get(arn)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("secret not found: {arn}"))
        }
    }

    fn make_client(secrets: HashMap<String, String>) -> SecretsManagerClient {
        SecretsManagerClient::with_fetcher(Box::new(FakeSecretFetcher { secrets }))
    }

    #[tokio::test]
    async fn arn_overwrites_direct_value() {
        let mut config = Config::default();
        config.server.auth.api_key.master_key = Some("direct-value".to_string());
        config.server.auth.api_key.master_key_arn =
            Some("arn:aws:secretsmanager:us-east-1:123:secret:key".to_string());

        let client = make_client(HashMap::from([(
            "arn:aws:secretsmanager:us-east-1:123:secret:key".to_string(),
            "arn-resolved-value".to_string(),
        )]));

        config.resolve_secrets_with(&client).await.unwrap();

        assert_eq!(
            config.server.auth.api_key.master_key.as_deref(),
            Some("arn-resolved-value")
        );
    }

    #[tokio::test]
    async fn arn_only_sets_master_key() {
        let mut config = Config::default();
        config.server.auth.api_key.master_key_arn =
            Some("arn:aws:secretsmanager:us-east-1:123:secret:key".to_string());

        let client = make_client(HashMap::from([(
            "arn:aws:secretsmanager:us-east-1:123:secret:key".to_string(),
            "arn-resolved-value".to_string(),
        )]));

        config.resolve_secrets_with(&client).await.unwrap();

        assert_eq!(
            config.server.auth.api_key.master_key.as_deref(),
            Some("arn-resolved-value")
        );
    }

    #[tokio::test]
    async fn direct_only_unchanged() {
        let mut config = Config::default();
        config.server.auth.api_key.master_key = Some("direct-value".to_string());

        let client = make_client(HashMap::new());

        config.resolve_secrets_with(&client).await.unwrap();

        assert_eq!(
            config.server.auth.api_key.master_key.as_deref(),
            Some("direct-value")
        );
    }

    #[tokio::test]
    async fn neither_set_remains_none() {
        let mut config = Config::default();

        let client = make_client(HashMap::new());

        config.resolve_secrets_with(&client).await.unwrap();

        assert!(config.server.auth.api_key.master_key.is_none());
    }

    #[cfg(feature = "postgres")]
    mod postgres_secrets {
        use super::*;

        fn pg_config(config: &mut Config) -> &mut PostgresConfig {
            config
                .backend
                .postgres
                .get_or_insert_with(PostgresConfig::default)
        }

        #[tokio::test]
        async fn rds_secrets_arn_builds_url_from_json() {
            let rds_json = r#"{"username":"admin","password":"s3cret","host":"mydb.cluster-abc.us-east-1.rds.amazonaws.com","port":5432,"dbname":"myapp"}"#;
            let mut config = Config::default();
            pg_config(&mut config).rds_secrets_arn = Some("arn:rds".to_string());

            let client = make_client(HashMap::from([(
                "arn:rds".to_string(),
                rds_json.to_string(),
            )]));

            config.resolve_secrets_with(&client).await.unwrap();

            assert_eq!(
                config.backend.postgres.as_ref().unwrap().url.as_deref(),
                Some(
                    "postgres://admin:s3cret@mydb.cluster-abc.us-east-1.rds.amazonaws.com:5432/myapp"
                )
            );
        }

        #[tokio::test]
        async fn rds_secrets_arn_defaults_port_and_dbname() {
            let rds_json = r#"{"username":"admin","password":"pass","host":"db.example.com"}"#;
            let mut config = Config::default();
            pg_config(&mut config).rds_secrets_arn = Some("arn:rds".to_string());

            let client = make_client(HashMap::from([(
                "arn:rds".to_string(),
                rds_json.to_string(),
            )]));

            config.resolve_secrets_with(&client).await.unwrap();

            assert_eq!(
                config.backend.postgres.as_ref().unwrap().url.as_deref(),
                Some("postgres://admin:pass@db.example.com:5432/postgres")
            );
        }

        #[tokio::test]
        async fn rds_secrets_arn_takes_priority_over_url_arn() {
            let rds_json = r#"{"username":"u","password":"p","host":"rds.example.com"}"#;
            let mut config = Config::default();
            let pg = pg_config(&mut config);
            pg.rds_secrets_arn = Some("arn:rds".to_string());
            pg.url_arn = Some("arn:url".to_string());

            let client = make_client(HashMap::from([
                ("arn:rds".to_string(), rds_json.to_string()),
                (
                    "arn:url".to_string(),
                    "postgres://from-url-arn/db".to_string(),
                ),
            ]));

            config.resolve_secrets_with(&client).await.unwrap();

            let url = config
                .backend
                .postgres
                .as_ref()
                .unwrap()
                .url
                .as_deref()
                .unwrap();
            assert!(
                url.contains("rds.example.com"),
                "should use RDS secret, got: {url}"
            );
        }

        #[tokio::test]
        async fn url_arn_still_works_without_rds_arn() {
            let mut config = Config::default();
            pg_config(&mut config).url_arn = Some("arn:url".to_string());

            let client = make_client(HashMap::from([(
                "arn:url".to_string(),
                "postgres://direct-url/db".to_string(),
            )]));

            config.resolve_secrets_with(&client).await.unwrap();

            assert_eq!(
                config.backend.postgres.as_ref().unwrap().url.as_deref(),
                Some("postgres://direct-url/db")
            );
        }

        #[tokio::test]
        async fn sslrootcert_appended_to_rds_url() {
            let rds_json = r#"{"username":"u","password":"p","host":"db.example.com","port":5432,"dbname":"app"}"#;
            let mut config = Config::default();
            let pg = pg_config(&mut config);
            pg.rds_secrets_arn = Some("arn:rds".to_string());
            pg.sslrootcert = Some("/etc/ssl/rds-ca.pem".to_string());

            let client = make_client(HashMap::from([(
                "arn:rds".to_string(),
                rds_json.to_string(),
            )]));

            config.resolve_secrets_with(&client).await.unwrap();

            assert_eq!(
                config.backend.postgres.as_ref().unwrap().url.as_deref(),
                Some(
                    "postgres://u:p@db.example.com:5432/app?sslmode=verify-full&sslrootcert=/etc/ssl/rds-ca.pem"
                )
            );
        }

        #[tokio::test]
        async fn rds_json_parse_error_has_clear_message() {
            let mut config = Config::default();
            pg_config(&mut config).rds_secrets_arn = Some("arn:bad".to_string());

            let client = make_client(HashMap::from([(
                "arn:bad".to_string(),
                "not-valid-json".to_string(),
            )]));

            let err = config.resolve_secrets_with(&client).await.unwrap_err();
            let msg = format!("{err:#}");
            assert!(
                msg.contains("failed to parse RDS JSON secret from arn:bad"),
                "error: {msg}"
            );
        }

        #[tokio::test]
        async fn rds_password_with_special_chars_is_encoded() {
            let rds_json =
                r#"{"username":"admin","password":"p@ss:w/rd?#","host":"db.example.com"}"#;
            let mut config = Config::default();
            pg_config(&mut config).rds_secrets_arn = Some("arn:rds".to_string());

            let client = make_client(HashMap::from([(
                "arn:rds".to_string(),
                rds_json.to_string(),
            )]));

            config.resolve_secrets_with(&client).await.unwrap();

            let url = config
                .backend
                .postgres
                .as_ref()
                .unwrap()
                .url
                .as_deref()
                .unwrap();
            // Password should be percent-encoded
            assert!(
                !url.contains("p@ss:w/rd?#"),
                "password should be encoded, got: {url}"
            );
            assert!(
                url.contains("p%40ss%3Aw%2Frd%3F%23"),
                "expected encoded password, got: {url}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn contract_action_hooks_empty_by_default() {
        let hooks = ContractActionHooks::default();
        assert!(hooks.is_empty());
        assert!(
            hooks
                .action_config("contract_add")
                .unwrap()
                .hooks
                .is_empty()
        );
    }

    #[test]
    fn contract_action_hooks_lookup_unknown_returns_none() {
        let hooks = ContractActionHooks::default();
        assert!(hooks.action_config("contract_unknown").is_none());
        assert!(hooks.action_config("task_add").is_none());
    }

    #[test]
    fn contract_action_hooks_is_empty_tracks_any_hook() {
        let mut hooks = ContractActionHooks::default();
        hooks.contract_add.hooks.insert(
            "h".into(),
            HookDef {
                command: "true".into(),
                when: HookWhen::Post,
                mode: HookMode::Async,
                on_failure: OnFailure::Abort,
                enabled: true,
                env_vars: vec![],
                on_result: None,
                prompt: None,
                timeout_secs: 30,
            },
        );
        assert!(!hooks.is_empty());
    }

    #[test]
    fn cli_contract_hooks_flatten_toml() {
        let toml_str = r#"
            browser = true

            [contract_add.hooks.ping]
            command = "echo added"

            [contract_delete.hooks.audit]
            command = "logger delete"
            mode = "async"
        "#;
        let cli: CliConfig = toml::from_str(toml_str).unwrap();
        assert!(cli.contract_hooks.contract_add.hooks.contains_key("ping"));
        assert!(
            cli.contract_hooks
                .contract_delete
                .hooks
                .contains_key("audit")
        );
    }

    #[test]
    fn project_user_metadata_field_action_hooks_lookup() {
        // ProjectActionHooks
        let proj = ProjectActionHooks::default();
        assert!(proj.is_empty());
        assert!(proj.action_config("project_create").is_some());
        assert!(proj.action_config("project_member_role_change").is_some());
        assert!(proj.action_config("task_add").is_none());

        // UserActionHooks
        let user = UserActionHooks::default();
        assert!(user.is_empty());
        assert!(user.action_config("user_create").is_some());
        assert!(user.action_config("user_session_revoke").is_some());
        assert!(user.action_config("project_create").is_none());

        // MetadataFieldActionHooks
        let mf = MetadataFieldActionHooks::default();
        assert!(mf.is_empty());
        assert!(mf.action_config("metadata_field_define").is_some());
        assert!(mf.action_config("metadata_field_remove").is_some());
        assert!(mf.action_config("user_create").is_none());
    }

    #[test]
    fn cli_project_user_metadata_field_hooks_flatten_toml() {
        let toml_str = r#"
            browser = true

            [task_update.hooks.audit_update]
            command = "logger task-update"

            [project_create.hooks.bootstrap]
            command = "echo project-created"

            [project_member_role_change.hooks.notify]
            command = "send-role-change"

            [user_api_key_issue.hooks.security]
            command = "log-key-issue"

            [user_session_revoke.hooks.audit]
            command = "log-session-revoke"

            [metadata_field_define.hooks.schema_check]
            command = "validate-schema"
        "#;
        let cli: CliConfig = toml::from_str(toml_str).unwrap();
        assert!(
            cli.hooks.task_update.hooks.contains_key("audit_update"),
            "task_update should bind via flatten on TaskActionHooks"
        );
        assert!(
            cli.project_hooks
                .project_create
                .hooks
                .contains_key("bootstrap")
        );
        assert!(
            cli.project_hooks
                .project_member_role_change
                .hooks
                .contains_key("notify")
        );
        assert!(
            cli.user_hooks
                .user_api_key_issue
                .hooks
                .contains_key("security")
        );
        assert!(
            cli.user_hooks
                .user_session_revoke
                .hooks
                .contains_key("audit")
        );
        assert!(
            cli.metadata_field_hooks
                .metadata_field_define
                .hooks
                .contains_key("schema_check")
        );
    }

    #[test]
    fn server_relay_remote_project_hooks_toml() {
        let toml_str = r#"
            host = "127.0.0.1"
            port = 3142

            [relay.project_create.hooks.notify]
            command = "echo new-project"

            [remote.user_create.hooks.audit]
            command = "log-user-create"

            [remote.metadata_field_remove.hooks.cleanup]
            command = "purge-metadata"
        "#;
        let server: ServerConfig = toml::from_str(toml_str).unwrap();
        assert!(
            server
                .relay
                .project_hooks
                .project_create
                .hooks
                .contains_key("notify")
        );
        assert!(
            server
                .remote
                .user_hooks
                .user_create
                .hooks
                .contains_key("audit")
        );
        assert!(
            server
                .remote
                .metadata_field_hooks
                .metadata_field_remove
                .hooks
                .contains_key("cleanup")
        );
    }

    #[test]
    fn hook_def_defaults() {
        let json = r#"{"command": "echo hi"}"#;
        let hook: HookDef = serde_json::from_str(json).unwrap();
        assert_eq!(hook.command, "echo hi");
        assert_eq!(hook.when, HookWhen::Post);
        assert_eq!(hook.mode, HookMode::Async);
        assert_eq!(hook.on_failure, OnFailure::Abort);
        assert!(hook.enabled);
        assert!(hook.env_vars.is_empty());
        assert!(hook.on_result.is_none());
        assert!(hook.prompt.is_none());
        assert_eq!(hook.timeout_secs, 30);
    }

    #[test]
    fn hook_def_full_toml() {
        let toml_str = r#"
            command = "do-thing"
            when = "pre"
            mode = "sync"
            on_failure = "warn"
            enabled = false
            on_result = "none"
            prompt = "Confirm?"
            timeout_secs = 60

            [[env_vars]]
            name = "WEBHOOK_URL"
            required = true
            description = "destination"

            [[env_vars]]
            name = "OPTIONAL"
            required = false
            default = "fallback"
        "#;
        let hook: HookDef = toml::from_str(toml_str).unwrap();
        assert_eq!(hook.command, "do-thing");
        assert_eq!(hook.when, HookWhen::Pre);
        assert_eq!(hook.mode, HookMode::Sync);
        assert_eq!(hook.on_failure, OnFailure::Warn);
        assert!(!hook.enabled);
        assert_eq!(hook.on_result, Some(OnResult::None));
        assert_eq!(hook.prompt.as_deref(), Some("Confirm?"));
        assert_eq!(hook.timeout_secs, 60);
        assert_eq!(hook.env_vars.len(), 2);
        assert_eq!(hook.env_vars[0].name, "WEBHOOK_URL");
        assert!(hook.env_vars[0].required);
        assert_eq!(hook.env_vars[1].name, "OPTIONAL");
        assert!(!hook.env_vars[1].required);
        assert_eq!(hook.env_vars[1].default.as_deref(), Some("fallback"));
    }

    #[test]
    fn cli_hooks_nested_toml() {
        let toml_str = r#"
            browser = true

            [remote]
            url = "http://api.senko.local:3141"

            [task_complete.hooks.webhook]
            command = "curl $WEBHOOK_URL"

            [[task_complete.hooks.webhook.env_vars]]
            name = "WEBHOOK_URL"

            [task_select.hooks.prompt_for_add]
            command = "echo none"
            on_result = "none"
        "#;
        let cli: CliConfig = toml::from_str(toml_str).unwrap();
        assert!(cli.browser);
        assert_eq!(
            cli.remote.url.as_deref(),
            Some("http://api.senko.local:3141")
        );
        let webhook = cli.hooks.task_complete.hooks.get("webhook").unwrap();
        assert_eq!(webhook.command, "curl $WEBHOOK_URL");
        assert_eq!(webhook.env_vars.len(), 1);
        assert_eq!(webhook.env_vars[0].name, "WEBHOOK_URL");
        let prompt = cli.hooks.task_select.hooks.get("prompt_for_add").unwrap();
        assert_eq!(prompt.on_result, Some(OnResult::None));
    }

    #[test]
    fn cli_task_resume_hooks_recognized() {
        let toml_str = r#"
            [task_resume.hooks.notify]
            command = "echo resumed"
            when = "post"
        "#;
        let cli: CliConfig = toml::from_str(toml_str).unwrap();
        let notify = cli.hooks.task_resume.hooks.get("notify").unwrap();
        assert_eq!(notify.command, "echo resumed");
        assert_eq!(
            cli.hooks
                .action_config("task_resume")
                .map(|c| c.hooks.len()),
            Some(1)
        );
    }

    #[test]
    fn server_relay_and_remote_hooks_toml() {
        let toml_str = r#"
            host = "127.0.0.1"
            port = 3142

            [relay]
            url = "http://relay-target"

            [relay.task_complete.hooks.audit]
            command = "logger"

            [remote.task_publish.hooks.metrics]
            command = "emit-metric"
        "#;
        let server: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(server.relay.url.as_deref(), Some("http://relay-target"));
        let audit = server.relay.hooks.task_complete.hooks.get("audit").unwrap();
        assert_eq!(audit.command, "logger");
        let metrics = server
            .remote
            .hooks
            .task_publish
            .hooks
            .get("metrics")
            .unwrap();
        assert_eq!(metrics.command, "emit-metric");
    }

    #[test]
    fn workflow_stages_user_extensible() {
        let toml_str = r#"
            merge_via = "direct"

            [task_add]
            default_dod = ["Write tests"]
            default_tags = ["backend"]
            default_priority = "p1"
            instructions = ["Be thorough"]

            [[task_add.metadata_fields]]
            key = "team"
            source = "value"
            value = "backend"

            [task_add.hooks.validate]
            command = "check-preconditions"
            when = "pre"
            mode = "sync"

            [my_custom_stage]
            instructions = ["do a thing"]
            custom_field = "hello"
        "#;
        let wf: WorkflowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(wf.merge_via, MergeVia::Direct);
        let add = wf.stage("task_add").unwrap();
        assert_eq!(add.instructions, vec!["Be thorough"]);
        assert_eq!(add.metadata_fields.len(), 1);
        assert_eq!(
            add.stage_field::<Vec<String>>("default_dod"),
            Some(vec!["Write tests".to_string()])
        );
        assert_eq!(
            add.stage_field::<String>("default_priority"),
            Some("p1".to_string())
        );
        let validate = add.hooks.get("validate").unwrap();
        assert_eq!(validate.when, HookWhen::Pre);
        assert_eq!(validate.mode, HookMode::Sync);

        let custom = wf.stage("my_custom_stage").unwrap();
        assert_eq!(custom.instructions, vec!["do a thing"]);
        assert_eq!(
            custom.stage_field::<String>("custom_field"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn metadata_field_source_env() {
        let json = r#"{"key": "sprint", "source": "env", "env_var": "SPRINT"}"#;
        let field: MetadataField = serde_json::from_str(json).unwrap();
        assert_eq!(field.key, "sprint");
        assert!(!field.required);
        assert!(field.default.is_none());
        match field.source {
            MetadataFieldSource::Env { env_var } => assert_eq!(env_var, "SPRINT"),
            _ => panic!("expected Env"),
        }
    }

    #[test]
    fn metadata_field_source_value() {
        let json = r#"{"key": "team", "source": "value", "value": "backend"}"#;
        let field: MetadataField = serde_json::from_str(json).unwrap();
        match field.source {
            MetadataFieldSource::Value { value } => assert_eq!(value, "backend"),
            _ => panic!("expected Value"),
        }
    }

    #[test]
    fn metadata_field_source_command() {
        let json = r#"{"key": "version", "source": "command", "command": "git describe --tags"}"#;
        let field: MetadataField = serde_json::from_str(json).unwrap();
        match field.source {
            MetadataFieldSource::Command { command } => {
                assert_eq!(command, "git describe --tags");
            }
            _ => panic!("expected Command"),
        }
    }

    #[test]
    fn metadata_field_source_prompt() {
        let json = r#"{"key": "priority", "source": "prompt", "prompt": "What priority?"}"#;
        let field: MetadataField = serde_json::from_str(json).unwrap();
        match field.source {
            MetadataFieldSource::Prompt { prompt } => assert_eq!(prompt, "What priority?"),
            _ => panic!("expected Prompt"),
        }
    }

    #[test]
    fn metadata_field_default_and_required() {
        let json = r#"{"key": "sprint", "source": "env", "env_var": "SPRINT", "default": "Q1", "required": true}"#;
        let field: MetadataField = serde_json::from_str(json).unwrap();
        assert_eq!(field.key, "sprint");
        assert!(field.required);
        assert_eq!(
            field.default,
            Some(serde_json::Value::String("Q1".to_string()))
        );
    }

    #[test]
    fn backend_sqlite_deser() {
        let toml_str = r#"
            [sqlite]
            db_path = "/data/senko.db"
        "#;
        let config: BackendConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.sqlite.db_path.as_deref(), Some("/data/senko.db"));
    }

    #[test]
    fn cli_remote_deser() {
        let toml_str = r#"
            [remote]
            url = "http://api.senko.local:3141"
            token = "sk_test"
        "#;
        let config: CliConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.remote.url.as_deref(),
            Some("http://api.senko.local:3141")
        );
        assert_eq!(config.remote.token.as_deref(), Some("sk_test"));
    }

    #[test]
    fn server_auth_deser() {
        let toml_str = r#"
            host = "127.0.0.1"
            port = 3142

            [auth.api_key]
            master_key = "sk_master"

            [auth.oidc]
            issuer_url = "https://auth.example.com"
            client_id = "my_client"
        "#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(config.port, Some(3142));
        assert_eq!(config.auth.api_key.master_key.as_deref(), Some("sk_master"));
        assert_eq!(
            config.auth.oidc.issuer_url.as_deref(),
            Some("https://auth.example.com")
        );
    }

    #[test]
    fn dev_bypass_round_trips_from_toml() {
        let raw: RawConfig = toml::from_str(
            r#"
            [server.auth.dev_bypass]
            enabled = true
            "#,
        )
        .unwrap();
        // Round-trip through merge (the easy-to-miss step) and resolve.
        let merged = RawConfig::default().merge(raw);
        let config = merged.resolve();
        assert!(config.server.auth.dev_bypass.enabled);
        assert!(config.server.auth.is_configured());
    }

    #[test]
    fn dev_bypass_default_is_disabled() {
        let config = RawConfig::default().resolve();
        assert!(!config.server.auth.dev_bypass.enabled);
        assert!(!config.server.auth.is_configured());
    }

    #[test]
    fn dev_bypass_exclusive_with_other_modes() {
        let mut config = Config::default();
        config.server.auth.api_key.master_key = Some("sk".into());
        config.server.auth.dev_bypass.enabled = true;
        let err = config.server.auth.validate_exclusive().unwrap_err();
        assert!(
            err.contains("dev_bypass"),
            "error mentions dev_bypass: {err}"
        );
    }

    #[test]
    fn apply_cli_dev_no_auth_enables_bypass() {
        let mut config = Config::default();
        config.apply_cli(&CliOverrides {
            dev_no_auth: true,
            ..Default::default()
        });
        assert!(config.server.auth.dev_bypass.enabled);
    }

    #[test]
    fn apply_cli_local_clears_remote_url() {
        let mut config = Config::default();
        config.cli.remote.url = Some("https://senko.example.com".into());
        config.cli.remote.token = Some("tok".into());
        config.apply_cli(&CliOverrides {
            local: true,
            ..Default::default()
        });
        assert!(config.cli.remote.url.is_none());
        assert!(config.cli.remote.token.is_none());
    }

    #[test]
    fn apply_cli_without_local_keeps_remote_url() {
        let mut config = Config::default();
        config.cli.remote.url = Some("https://senko.example.com".into());
        config.apply_cli(&CliOverrides::default());
        assert_eq!(
            config.cli.remote.url.as_deref(),
            Some("https://senko.example.com")
        );
    }

    #[test]
    fn raw_config_merge_stages() {
        let base: RawConfig = toml::from_str(
            r#"
            [workflow.task_start]
            instructions = ["base instruction"]
            [workflow.task_plan]
            required_sections = ["Context"]
        "#,
        )
        .unwrap();

        let overlay: RawConfig = toml::from_str(
            r#"
            [workflow.task_start]
            instructions = ["overlay instruction"]
        "#,
        )
        .unwrap();

        let merged = base.merge(overlay);
        let resolved = merged.resolve();
        let start = resolved.workflow.stage("task_start").unwrap();
        assert_eq!(start.instructions, vec!["overlay instruction"]);
        let plan = resolved.workflow.stage("task_plan").unwrap();
        assert_eq!(
            plan.stage_field::<Vec<String>>("required_sections"),
            Some(vec!["Context".to_string()])
        );
    }

    #[test]
    fn resolve_full() {
        let raw: RawConfig = toml::from_str(
            r#"
            [project]
            name = "test"

            [backend.sqlite]
            db_path = "/tmp/test.db"

            [cli.remote]
            url = "http://localhost:3141"

            [server]
            host = "127.0.0.1"
            port = 3142

            [server.auth.api_key]
            master_key = "secret"

            [workflow]
            merge_via = "pr"
            branch_template = "feat/{id}"

            [workflow.task_start]
            instructions = ["Check prerequisites"]
        "#,
        )
        .unwrap();

        let config = raw.resolve();
        assert_eq!(config.project.name.as_deref(), Some("test"));
        assert_eq!(
            config.backend.sqlite.db_path.as_deref(),
            Some("/tmp/test.db")
        );
        assert_eq!(
            config.cli.remote.url.as_deref(),
            Some("http://localhost:3141")
        );
        assert_eq!(config.server.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(config.server.port, Some(3142));
        assert_eq!(
            config.server.auth.api_key.master_key.as_deref(),
            Some("secret")
        );
        assert_eq!(config.workflow.merge_via, MergeVia::Pr);
        assert_eq!(
            config.workflow.branch_template.as_deref(),
            Some("feat/{id}")
        );
        assert_eq!(
            config.workflow.stage("task_start").unwrap().instructions,
            vec!["Check prerequisites"]
        );
        // Defaults
        assert!(config.workflow.auto_merge);
        assert_eq!(config.workflow.branch_mode, BranchMode::default());
        assert_eq!(config.workflow.branch_mode.kind, BranchModeType::Worktree);
        assert!(config.workflow.branch_mode.create);
        assert_eq!(config.workflow.merge_strategy, MergeStrategy::Rebase);
    }

    #[test]
    fn apply_env_does_not_touch_hooks() {
        // Ensure removed env vars (SENKO_HOOKS_ENABLED etc.) do not panic.
        let mut config = Config::default();
        config.apply_env();
        assert!(config.cli.hooks.is_empty());
    }

    #[test]
    fn env_override_cli_remote_url() {
        let mut config = Config::default();
        // SAFETY: Tests may run in parallel; key is test-local.
        unsafe {
            std::env::set_var("SENKO_CLI_REMOTE_URL", "http://remote:3142");
        }
        config.apply_env();
        assert_eq!(
            config.cli.remote.url,
            Some("http://remote:3142".to_string())
        );
        unsafe {
            std::env::remove_var("SENKO_CLI_REMOTE_URL");
        }
    }

    #[test]
    fn server_port_helpers() {
        let mut config = Config::default();
        assert_eq!(config.server_port_or(3142), 3142);
        assert!(!config.server_port_is_explicit());

        config.server.port = Some(4000);
        assert_eq!(config.server_port_or(3142), 4000);
        assert!(config.server_port_is_explicit());
    }

    #[test]
    fn effective_server_host_default() {
        let config = Config::default();
        assert_eq!(config.effective_server_host(), "127.0.0.1");
    }

    #[test]
    fn effective_server_host_custom() {
        let mut config = Config::default();
        config.server.host = Some("0.0.0.0".to_string());
        assert_eq!(config.effective_server_host(), "0.0.0.0");
    }

    #[test]
    fn apply_cli_server_overrides() {
        let mut config = Config::default();
        config.apply_cli(&CliOverrides {
            server_port: Some(5000),
            server_host: Some("10.0.0.1".to_string()),
            ..Default::default()
        });
        assert_eq!(config.server.port, Some(5000));
        assert_eq!(config.server.host.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn cli_browser_deserialize_false() {
        let raw: RawConfig = toml::from_str(
            r#"
            [cli]
            browser = false
        "#,
        )
        .unwrap();
        assert_eq!(raw.cli.browser, Some(false));
    }

    #[test]
    fn cli_browser_default_true() {
        let raw: RawConfig = toml::from_str("").unwrap();
        let config = raw.resolve();
        assert!(config.cli.browser);
    }

    #[test]
    fn cli_browser_merge_overlay_wins() {
        let base: RawConfig = toml::from_str(
            r#"
            [cli]
            browser = true
        "#,
        )
        .unwrap();
        let overlay: RawConfig = toml::from_str(
            r#"
            [cli]
            browser = false
        "#,
        )
        .unwrap();
        let merged = base.merge(overlay);
        assert_eq!(merged.cli.browser, Some(false));
    }

    // --- BranchMode deserialization / env / overlay tests ---

    fn parse_branch_mode(toml_str: &str) -> BranchMode {
        let raw: RawConfig = toml::from_str(toml_str).unwrap();
        raw.resolve().workflow.branch_mode
    }

    #[test]
    fn branch_mode_legacy_string_worktree() {
        let bm = parse_branch_mode(
            r#"
            [workflow]
            branch_mode = "worktree"
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Worktree);
        assert!(bm.create);
    }

    #[test]
    fn branch_mode_legacy_string_branch() {
        let bm = parse_branch_mode(
            r#"
            [workflow]
            branch_mode = "branch"
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Branch);
        assert!(bm.create);
    }

    #[test]
    fn branch_mode_modern_worktree_create_true() {
        let bm = parse_branch_mode(
            r#"
            [workflow.branch_mode]
            type = "worktree"
            create = true
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Worktree);
        assert!(bm.create);
    }

    #[test]
    fn branch_mode_modern_worktree_create_false() {
        let bm = parse_branch_mode(
            r#"
            [workflow.branch_mode]
            type = "worktree"
            create = false
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Worktree);
        assert!(!bm.create);
    }

    #[test]
    fn branch_mode_modern_branch_create_true() {
        let bm = parse_branch_mode(
            r#"
            [workflow.branch_mode]
            type = "branch"
            create = true
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Branch);
        assert!(bm.create);
    }

    #[test]
    fn branch_mode_modern_branch_create_false() {
        let bm = parse_branch_mode(
            r#"
            [workflow.branch_mode]
            type = "branch"
            create = false
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Branch);
        assert!(!bm.create);
    }

    #[test]
    fn branch_mode_modern_create_defaults_to_true() {
        let bm = parse_branch_mode(
            r#"
            [workflow.branch_mode]
            type = "branch"
        "#,
        );
        assert_eq!(bm.kind, BranchModeType::Branch);
        assert!(bm.create);
    }

    #[test]
    #[serial(branch_mode_env)]
    fn branch_mode_env_type_overrides() {
        let mut config = Config::default();
        // SAFETY: env vars are test-local; serial guard prevents races.
        unsafe {
            std::env::set_var("SENKO_BRANCH_MODE_TYPE", "branch");
        }
        config.apply_env();
        unsafe {
            std::env::remove_var("SENKO_BRANCH_MODE_TYPE");
        }
        assert_eq!(config.workflow.branch_mode.kind, BranchModeType::Branch);
        assert!(config.workflow.branch_mode.create);
    }

    #[test]
    #[serial(branch_mode_env)]
    fn branch_mode_env_create_overrides() {
        let mut config = Config::default();
        // SAFETY: env vars are test-local; serial guard prevents races.
        unsafe {
            std::env::set_var("SENKO_BRANCH_MODE_CREATE", "false");
        }
        config.apply_env();
        unsafe {
            std::env::remove_var("SENKO_BRANCH_MODE_CREATE");
        }
        assert_eq!(config.workflow.branch_mode.kind, BranchModeType::Worktree);
        assert!(!config.workflow.branch_mode.create);
    }

    #[test]
    #[serial(branch_mode_env)]
    fn branch_mode_legacy_env_var_is_removed() {
        // SENKO_BRANCH_MODE (the pre-split variable) must no longer take effect.
        let mut config = Config::default();
        // SAFETY: env vars are test-local; serial guard prevents races.
        unsafe {
            std::env::set_var("SENKO_BRANCH_MODE", "branch");
        }
        config.apply_env();
        unsafe {
            std::env::remove_var("SENKO_BRANCH_MODE");
        }
        // Default unchanged because the legacy variable is no longer consulted.
        assert_eq!(config.workflow.branch_mode, BranchMode::default());
    }

    #[test]
    fn branch_mode_overlay_replaces_base() {
        let base: RawConfig = toml::from_str(
            r#"
            [workflow.branch_mode]
            type = "worktree"
            create = true
        "#,
        )
        .unwrap();
        let overlay: RawConfig = toml::from_str(
            r#"
            [workflow.branch_mode]
            type = "branch"
            create = false
        "#,
        )
        .unwrap();
        let merged = base.merge(overlay).resolve();
        assert_eq!(merged.workflow.branch_mode.kind, BranchModeType::Branch);
        assert!(!merged.workflow.branch_mode.create);
    }
}
