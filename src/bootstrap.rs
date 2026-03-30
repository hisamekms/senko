use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::application::port::HookExecutor;
use crate::application::{ProjectService, TaskService, UserService};
use crate::domain::repository::TaskBackend;
use crate::domain::config::{Config, HookMode, LogConfig, LogFormat};
use crate::infra::http::HttpBackend;
use crate::infra::hook::executor::ShellHookExecutor;
use crate::infra::hook::{RuntimeMode, BackendInfo};
use crate::infra::pr_verifier::GhCliPrVerifier;

pub const DEFAULT_PROJECT_ID: i64 = 1;
pub const DEFAULT_USER_ID: i64 = 1;

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
) -> Arc<dyn HookExecutor> {
    let should_fire = should_fire_client_hooks(&config, using_http);
    Arc::new(ShellHookExecutor::new(config, should_fire, runtime_mode, backend_info))
}

pub fn create_task_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    using_http: bool,
    project_root: &Path,
) -> TaskService {
    let backend_info = resolve_backend_info(config, project_root);
    let hooks = create_hook_executor(config.clone(), using_http, RuntimeMode::Cli, backend_info);
    let pr_verifier = Arc::new(GhCliPrVerifier);
    TaskService::new(backend, hooks, pr_verifier, config.workflow.clone())
}

pub fn create_project_service(backend: Arc<dyn TaskBackend>) -> ProjectService {
    ProjectService::new(backend)
}

pub fn create_user_service(backend: Arc<dyn TaskBackend>) -> UserService {
    UserService::new(backend)
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
