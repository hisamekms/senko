use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::application::port::auth::AuthProvider;
use crate::application::port::{HookExecutor, NoOpPrVerifier, PrVerifier};
use crate::application::{HookTestService, ProjectService, TaskService, UserService};
use crate::domain::task::CompletionPolicy;
use crate::application::port::TaskBackend;
use crate::infra::config::{Config, HookMode, LogConfig, LogFormat, RawConfig};
use crate::infra::http::HttpBackend;
use crate::infra::hook::executor::ShellHookExecutor;
use crate::infra::hook::test_executor::ShellHookTestExecutor;
use crate::infra::hook::{RuntimeMode, BackendInfo};
use crate::infra::auth::ApiKeyProvider;
use crate::infra::pr_verifier::GhCliPrVerifier;

// Re-exports for presentation layer (avoid direct infra dependency)
pub use crate::infra::hook;
pub use crate::infra::project_root::resolve_project_root;

pub use crate::domain::{DEFAULT_PROJECT_ID, DEFAULT_USER_ID};

/// Create the appropriate backend based on config (env + CLI already applied).
/// Returns (backend, is_http) where is_http indicates HTTP mode for hook control.
pub fn create_backend(
    project_root: &Path,
    config: &Config,
) -> Result<(Arc<dyn TaskBackend>, bool)> {
    // 1. HTTP backend (api_url from env or config.toml)
    if let Some(ref url) = config.backend.api_url {
        let backend = match config.backend.api_key.as_ref() {
            Some(key) => HttpBackend::with_api_key(url, key.clone()),
            None => HttpBackend::new(url),
        };
        return Ok((Arc::new(backend), true));
    }

    // 2. DynamoDB backend
    #[cfg(feature = "dynamodb")]
    {
        use crate::infra::dynamodb::DynamoDbBackend;

        if let Some(ref ddb_config) = config.backend.dynamodb {
            if let Some(ref table_name) = ddb_config.table_name {
                return Ok((
                    Arc::new(DynamoDbBackend::new(
                        table_name.clone(),
                        ddb_config.region.clone(),
                    )),
                    false,
                ));
            }
        }
    }

    // 3. PostgreSQL backend
    #[cfg(feature = "postgres")]
    {
        use crate::infra::postgres::PostgresBackend;

        if let Some(ref pg_config) = config.backend.postgres {
            if let Some(ref database_url) = pg_config.url {
                return Ok((Arc::new(PostgresBackend::new(database_url.clone())), false));
            }
        }
    }

    // 4. Default: SqliteBackend
    let sqlite = crate::infra::sqlite::SqliteBackend::new(
        project_root,
        None,
        config.storage.db_path.as_deref(),
    )?;
    sqlite.sync_config_defaults(config)?;
    Ok((Arc::new(sqlite), false))
}

pub fn should_fire_client_hooks(config: &Config, using_http: bool) -> bool {
    match config.backend.hook_mode {
        HookMode::Server => !using_http,
        HookMode::Client | HookMode::Both => true,
    }
}

/// Resolve the backend info from config for hook envelope metadata.
/// Mirrors the priority logic of `create_backend`.
pub fn resolve_backend_info(config: &Config, project_root: &Path) -> BackendInfo {
    if let Some(ref url) = config.backend.api_url {
        return BackendInfo::Http { api_url: url.clone() };
    }
    #[cfg(feature = "dynamodb")]
    if config.backend.dynamodb.as_ref().and_then(|d| d.table_name.as_ref()).is_some() {
        return BackendInfo::Dynamodb;
    }
    #[cfg(feature = "postgres")]
    if config.backend.postgres.as_ref().and_then(|p| p.url.as_ref()).is_some() {
        return BackendInfo::Postgresql;
    }
    let db_path = crate::infra::sqlite::resolve_db_path_preview(project_root, config.storage.db_path.as_deref())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    BackendInfo::Sqlite { db_file_path: db_path }
}

pub fn create_hook_executor(
    config: Config,
    using_http: bool,
    runtime_mode: RuntimeMode,
    backend_info: BackendInfo,
    backend: Arc<dyn TaskBackend>,
) -> Arc<dyn HookExecutor> {
    let should_fire = should_fire_client_hooks(&config, using_http);
    Arc::new(ShellHookExecutor::new(config, should_fire, runtime_mode, backend_info, backend))
}

pub fn create_api_hook_executor(
    config: Config,
    backend_info: BackendInfo,
    backend: Arc<dyn TaskBackend>,
) -> Arc<dyn HookExecutor> {
    // API server always fires hooks
    Arc::new(ShellHookExecutor::new(config, true, RuntimeMode::Api, backend_info, backend))
}

pub fn create_pr_verifier() -> Arc<dyn crate::application::port::PrVerifier> {
    Arc::new(GhCliPrVerifier)
}

pub fn create_auth_provider(
    config: &Config,
    backend: Arc<dyn TaskBackend>,
) -> Option<Arc<dyn AuthProvider>> {
    if config.auth.enabled {
        if backend.supports_api_key_auth() {
            tracing::info!("authentication enabled");
            Some(Arc::new(ApiKeyProvider::new(backend)))
        } else {
            tracing::warn!(
                "authentication requested but backend does not support API key auth; disabling"
            );
            None
        }
    } else {
        tracing::info!("authentication disabled");
        None
    }
}

pub fn create_task_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    using_http: bool,
    project_root: &Path,
) -> TaskService {
    let backend_info = resolve_backend_info(config, project_root);
    let hooks = create_hook_executor(config.clone(), using_http, RuntimeMode::Cli, backend_info, backend.clone());
    let pr_verifier: Arc<dyn PrVerifier> = if using_http {
        Arc::new(NoOpPrVerifier)
    } else {
        Arc::new(GhCliPrVerifier)
    };
    let completion_policy = CompletionPolicy::new(config.workflow.completion_mode, config.workflow.auto_merge);
    TaskService::new(backend, hooks, pr_verifier, completion_policy)
}

pub fn create_project_service(backend: Arc<dyn TaskBackend>) -> ProjectService {
    ProjectService::new(backend)
}

pub fn create_user_service(backend: Arc<dyn TaskBackend>) -> UserService {
    UserService::new(backend)
}

pub fn create_hook_test_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    project_root: &Path,
) -> HookTestService {
    let backend_info = resolve_backend_info(config, project_root);
    let hook_test = Arc::new(ShellHookTestExecutor::new(
        config.clone(),
        RuntimeMode::Cli,
        backend_info,
        backend.clone(),
    ));
    HookTestService::new(backend, hook_test)
}

/// Resolve the project ID from config (CLI > env > config.toml already applied).
pub async fn resolve_project_id(
    backend: &dyn TaskBackend,
    config: &Config,
) -> Result<i64> {
    match config.project.name.as_deref() {
        Some(n) => {
            let project = backend
                .get_project_by_name(n)
                .await
                .with_context(|| format!("project not found: {n}"))?;
            Ok(project.id())
        }
        None => Ok(DEFAULT_PROJECT_ID),
    }
}

/// Resolve the user ID from config (CLI > env > config.toml already applied).
pub async fn resolve_user_id(
    backend: &dyn TaskBackend,
    config: &Config,
) -> Result<i64> {
    match config.user.name.as_deref() {
        Some(n) => {
            let user = backend
                .get_user_by_username(n)
                .await
                .with_context(|| format!("user not found: {n}"))?;
            Ok(user.id())
        }
        None => Ok(DEFAULT_USER_ID),
    }
}

pub fn init_tracing(config: &LogConfig) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    let registry = tracing_subscriber::registry().with(env_filter);

    match config.format {
        LogFormat::Json => {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}

pub fn load_config(project_root: &Path, explicit_config: Option<&Path>) -> Result<Config> {
    // 1. Load user config (lowest priority layer)
    let user_raw = load_user_config()?;

    // 2. Determine and load the project/explicit config
    let project_raw = if let Some(path) = explicit_config {
        // Explicit --config flag: must exist
        Some(load_config_file(path, true)?)
    } else if let Some(env_path) = env_config_path() {
        // SENKO_CONFIG env var: must exist
        Some(load_config_file(&env_path, true)?)
    } else {
        let default_path = project_root.join(".senko").join("config.toml");
        if default_path.exists() {
            Some(load_config_file(&default_path, false)?)
        } else {
            None
        }
    };

    // 3. Merge: user config as base, project config as overlay
    let merged_raw = match (user_raw, project_raw) {
        (Some(base), Some(overlay)) => base.merge(overlay),
        (None, Some(overlay)) => overlay,
        (Some(base), None) => base,
        (None, None) => RawConfig::default(),
    };

    // 4. Resolve to final Config and apply env overrides
    let mut config = merged_raw.resolve();
    config.apply_env();
    Ok(config)
}

/// Return the user-level config path.
/// `$XDG_CONFIG_HOME/senko/config.toml` or `~/.config/senko/config.toml`
fn user_config_path() -> Option<PathBuf> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(config_dir.join("senko").join("config.toml"))
}

/// Load user-level config if it exists.
fn load_user_config() -> Result<Option<RawConfig>> {
    let path = match user_config_path() {
        Some(p) if p.exists() => p,
        _ => return Ok(None),
    };
    let raw = load_config_file(&path, false)?;
    Ok(Some(raw))
}

/// Return the config path from the SENKO_CONFIG env var, if set.
fn env_config_path() -> Option<PathBuf> {
    std::env::var("SENKO_CONFIG")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Load and parse a config file into RawConfig, with legacy hook format detection.
fn load_config_file(path: &Path, must_exist: bool) -> Result<RawConfig> {
    if !path.exists() {
        if must_exist {
            bail!("config file not found: {}", path.display());
        }
        return Ok(RawConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    detect_legacy_hook_format(&content, path)?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))
}

/// Check if the config uses the old array-based hook format and return a helpful error.
fn detect_legacy_hook_format(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Ok(()), // let the real parser produce the error
    };
    if let Some(hooks) = raw.get("hooks").and_then(|v| v.as_table()) {
        for (key, val) in hooks {
            if val.is_str() || val.is_array() {
                bail!(
                    "Legacy hook format detected in {}.\n\
                     The array-based hook format is no longer supported.\n\
                     Please migrate to named hooks:\n\n\
                     Old format:\n  [hooks]\n  {} = \"command\"\n\n\
                     New format:\n  [hooks.{}.my-hook]\n  command = \"command\"\n",
                    path.display(),
                    key,
                    key,
                );
            }
        }
    }
    Ok(())
}
