use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::application::port::HookExecutor;
use crate::application::{ProjectService, TaskService, UserService};
use crate::domain::repository::TaskBackend;
use crate::domain::config::{Config, HookMode, LogConfig, LogFormat};
use crate::infra::hook as hooks;
use crate::infra::http::HttpBackend;
use crate::infra::hook::executor::ShellHookExecutor;
use crate::infra::pr_verifier::GhCliPrVerifier;

pub const DEFAULT_PROJECT_ID: i64 = 1;
pub const DEFAULT_USER_ID: i64 = 1;

/// Create the appropriate backend based on env var / config.
/// Returns (backend, is_http) where is_http indicates HTTP mode for hook control.
pub fn create_backend(
    project_root: &Path,
    config_path: Option<&Path>,
    db_path: Option<&Path>,
    #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
    postgres_url: Option<&str>,
) -> Result<(Arc<dyn TaskBackend>, bool)> {
    let resolve_api_key = |config: &Config| -> Option<String> {
        std::env::var("SENKO_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| config.backend.api_key.clone())
    };

    // 1. SENKO_API_URL env var takes priority
    if let Ok(url) = std::env::var("SENKO_API_URL") {
        if !url.is_empty() {
            let config = hooks::load_config(project_root, config_path)?;
            let backend = match resolve_api_key(&config) {
                Some(key) => HttpBackend::with_api_key(&url, key),
                None => HttpBackend::new(&url),
            };
            return Ok((Arc::new(backend), true));
        }
    }

    // 2. config.toml [backend] api_url
    let config = hooks::load_config(project_root, config_path)?;
    if let Some(ref url) = config.backend.api_url {
        let backend = match resolve_api_key(&config) {
            Some(key) => HttpBackend::with_api_key(url, key),
            None => HttpBackend::new(url),
        };
        return Ok((Arc::new(backend), true));
    }

    // 3. DynamoDB backend (via env var or config)
    #[cfg(feature = "dynamodb")]
    {
        use crate::infra::dynamodb::DynamoDbBackend;

        let table_from_env = std::env::var("SENKO_DYNAMODB_TABLE").ok().filter(|s| !s.is_empty());
        let region_from_env = std::env::var("SENKO_DYNAMODB_REGION").ok().filter(|s| !s.is_empty());

        let (table, region) = match (&table_from_env, &config.backend.dynamodb) {
            (Some(t), _) => (Some(t.clone()), region_from_env),
            (None, Some(ddb_config)) => {
                let table = ddb_config.table_name.clone();
                let region = region_from_env.or_else(|| ddb_config.region.clone());
                (table, region)
            }
            _ => (None, None),
        };

        if let Some(table_name) = table {
            return Ok((Arc::new(DynamoDbBackend::new(table_name, region)), false));
        }
    }

    // 4. PostgreSQL backend (via CLI arg, env var, or config)
    #[cfg(feature = "postgres")]
    {
        use crate::infra::postgres::PostgresBackend;

        // Priority: CLI --postgres-url > SENKO_POSTGRES_URL env > config.toml
        let url = postgres_url
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::var("SENKO_POSTGRES_URL")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                config.backend.postgres.as_ref().and_then(|pg| pg.url.clone())
            });

        if let Some(database_url) = url {
            return Ok((Arc::new(PostgresBackend::new(database_url)), false));
        }
    }

    // 5. Default: SqliteBackend
    Ok((Arc::new(crate::infra::sqlite::SqliteBackend::new(project_root, db_path, config.storage.db_path.as_deref())?), false))
}

pub fn load_config_with_overrides(
    root: &Path,
    config_path: Option<&Path>,
    log_dir: Option<&Path>,
) -> Result<Config> {
    let mut config = hooks::load_config(root, config_path)?;
    if let Some(d) = log_dir {
        config.log.dir = Some(d.to_string_lossy().into_owned());
    }
    Ok(config)
}

pub fn should_fire_client_hooks(config: &Config, using_http: bool) -> bool {
    match config.backend.hook_mode {
        HookMode::Server => !using_http,
        HookMode::Client | HookMode::Both => true,
    }
}

pub fn create_hook_executor(config: Config, using_http: bool) -> Arc<dyn HookExecutor> {
    let should_fire = should_fire_client_hooks(&config, using_http);
    Arc::new(ShellHookExecutor::new(config, should_fire))
}

pub fn create_task_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    using_http: bool,
) -> TaskService {
    let hooks = create_hook_executor(config.clone(), using_http);
    let pr_verifier = Arc::new(GhCliPrVerifier);
    TaskService::new(backend, hooks, pr_verifier, config.workflow.clone())
}

pub fn create_project_service(backend: Arc<dyn TaskBackend>) -> ProjectService {
    ProjectService::new(backend)
}

pub fn create_user_service(backend: Arc<dyn TaskBackend>) -> UserService {
    UserService::new(backend)
}

/// Resolve the project ID from CLI flag, config, or default.
///
/// Priority: CLI flag / SENKO_PROJECT env > config.toml [project] name > DEFAULT_PROJECT_ID
pub async fn resolve_project_id(
    backend: &dyn TaskBackend,
    cli_project: Option<&str>,
    config: &Config,
) -> Result<i64> {
    let name = cli_project.or(config.project.name.as_deref());
    match name {
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

/// Resolve the user ID from CLI flag, config, or default.
///
/// Priority: --user / SENKO_USER env > config.toml [user] name > DEFAULT_USER_ID
pub async fn resolve_user_id(
    backend: &dyn TaskBackend,
    cli_user: Option<&str>,
    config: &Config,
) -> Result<i64> {
    let name = cli_user.or(config.user.name.as_deref());
    match name {
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
