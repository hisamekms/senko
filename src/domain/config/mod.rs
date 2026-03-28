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

mod string_or_vec {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrVec {
            String(String),
            Vec(Vec<String>),
        }
        match StringOrVec::deserialize(deserializer)? {
            StringOrVec::String(s) => Ok(vec![s]),
            StringOrVec::Vec(v) => Ok(v),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_added: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_ready: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_started: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_completed: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_canceled: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_no_eligible_task: Vec<String>,
}
