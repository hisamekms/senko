use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, bail};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use crate::application::port::TaskBackend;
use crate::application::port::auth::AuthProvider;
use crate::application::port::{HookDataSource, HookExecutor, PrVerifier};
use crate::application::{
    HookTestService, LocalContractOperations, LocalTaskOperations, MetadataFieldService,
    ProjectOperations, ProjectService, TaskOperations, UserService,
};
use crate::domain::task::CompletionPolicy;
use crate::infra::auth::{ApiKeyProvider, JwtAuthProvider, TrustedHeadersAuthProvider};
use crate::infra::config::{Config, LogConfig, LogFormat, RawConfig};
use crate::infra::hook::executor::ShellHookExecutor;
use crate::infra::hook::test_executor::ShellHookTestExecutor;
use crate::infra::hook::{
    BackendInfo, RuntimeMode, validate_config_hooks, warn_about_mismatched_runtime_sections,
};
use crate::infra::http::remote_contract_ops::RemoteContractOperations;
use crate::infra::http::remote_hook_data::RemoteHookDataSource;
use crate::infra::http::remote_metadata_field_ops::RemoteMetadataFieldOperations;
use crate::infra::http::remote_project_ops::RemoteProjectOperations;
use crate::infra::http::remote_task_ops::RemoteTaskOperations;
use crate::infra::http::remote_user_ops::RemoteUserOperations;
use crate::infra::http::trace_propagation::{merge_attributes, parse_otel_resource_attributes};
use crate::infra::pr_verifier::GhCliPrVerifier;
use crate::infra::xdg::XdgDirs;

/// Per-process UUIDv4 identifying one CLI invocation. Auto-generated on first
/// access and cached for the lifetime of the process, so every outbound HTTP
/// request in the same `senko …` invocation shares the same
/// `senko.operation.id` baggage value — letting the Remote correlate the
/// multiple API calls a single operation fans out into.
fn auto_operation_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| Uuid::new_v4().to_string())
}

/// Build the lowest-priority "auto" trace-attribute source.
///
/// Kept as a small helper so future auto-populated attributes can be added
/// here without touching every call site. Currently emits just
/// `senko.operation.id = <uuid>`.
fn auto_trace_attributes() -> Vec<(String, String)> {
    vec![(
        "senko.operation.id".to_string(),
        auto_operation_id().to_string(),
    )]
}

/// Resolve the final W3C Baggage attribute map from the four sources defined
/// by the OTel client-tracing contract, in precedence order:
/// `cli_attrs` (CLI `--attr`) > `SENKO_TRACE_ATTRIBUTES` >
/// `OTEL_RESOURCE_ATTRIBUTES` > auto-populated (`senko.operation.id`)
/// (with reserved namespaces filtered out of the OTel env source only).
///
/// `cli_attrs` is already parsed at clap time; env vars are parsed here via
/// [`parse_otel_resource_attributes`] so malformed entries are silently
/// skipped per OTel spec.
pub fn resolve_trace_attributes(cli_attrs: &[(String, String)]) -> BTreeMap<String, String> {
    let senko_env = std::env::var("SENKO_TRACE_ATTRIBUTES")
        .ok()
        .map(|s| parse_otel_resource_attributes(&s))
        .unwrap_or_default();
    let otel_env = std::env::var("OTEL_RESOURCE_ATTRIBUTES")
        .ok()
        .map(|s| parse_otel_resource_attributes(&s))
        .unwrap_or_default();
    merge_attributes(
        cli_attrs.to_vec(),
        senko_env,
        otel_env,
        auto_trace_attributes(),
    )
}

// Re-exports for presentation layer (avoid direct infra dependency)
pub use crate::infra::hook;
pub use crate::infra::project_root::resolve_project_root;

pub use crate::domain::project::ProjectId;
pub use crate::domain::{DEFAULT_PROJECT_ID, DEFAULT_USER_ID};

/// Create the appropriate backend based on config (env + CLI already applied).
///
/// Returns a local database backend (SQLite / PostgreSQL).
/// Remote HTTP mode is handled separately via `Remote*Operations`.
pub fn create_backend(project_root: &Path, config: &Config) -> Result<Arc<dyn TaskBackend>> {
    #[cfg(feature = "postgres")]
    {
        use crate::infra::postgres::PostgresBackend;

        if let Some(ref pg_config) = config.backend.postgres {
            if let Some(ref database_url) = pg_config.url {
                return Ok(Arc::new(PostgresBackend::new(
                    database_url.clone(),
                    pg_config.max_connections,
                )));
            }
        }
    }

    let sqlite = crate::infra::sqlite::SqliteBackend::new(
        project_root,
        None,
        config.backend.sqlite.db_path.as_deref(),
        &config.xdg,
    )?;
    sqlite.sync_config_defaults(config)?;
    Ok(Arc::new(sqlite))
}

/// Resolve the backend info from config for hook envelope metadata.
/// Mirrors the priority logic of `create_backend`.
pub fn resolve_backend_info(config: &Config, project_root: &Path) -> BackendInfo {
    if let Some(ref url) = config.cli.remote.url {
        return BackendInfo::Http {
            api_url: url.clone(),
        };
    }
    if let Some(ref url) = config.server.relay.url {
        return BackendInfo::Http {
            api_url: url.clone(),
        };
    }
    #[cfg(feature = "postgres")]
    if config
        .backend
        .postgres
        .as_ref()
        .and_then(|p| p.url.as_ref())
        .is_some()
    {
        return BackendInfo::Postgresql;
    }
    let db_path = crate::infra::sqlite::resolve_db_path_preview(
        project_root,
        config.backend.sqlite.db_path.as_deref(),
        &config.xdg,
    )
    .map(|p| p.display().to_string())
    .unwrap_or_else(|| "<unknown>".to_string());
    BackendInfo::Sqlite {
        db_file_path: db_path,
    }
}

pub fn create_hook_executor(
    config: Config,
    runtime_mode: RuntimeMode,
    backend_info: BackendInfo,
    hook_data: Arc<dyn HookDataSource>,
) -> Arc<dyn HookExecutor> {
    validate_config_hooks(&config);
    warn_about_mismatched_runtime_sections(&config, &runtime_mode);
    Arc::new(ShellHookExecutor::new(
        config,
        runtime_mode,
        backend_info,
        hook_data,
    ))
}

pub fn create_server_hook_executor(
    config: Config,
    runtime_mode: RuntimeMode,
    backend_info: BackendInfo,
    hook_data: Arc<dyn HookDataSource>,
) -> Arc<dyn HookExecutor> {
    validate_config_hooks(&config);
    warn_about_mismatched_runtime_sections(&config, &runtime_mode);
    Arc::new(ShellHookExecutor::new(
        config,
        runtime_mode,
        backend_info,
        hook_data,
    ))
}

pub fn create_pr_verifier() -> Arc<dyn crate::application::port::PrVerifier> {
    Arc::new(GhCliPrVerifier)
}

/// Active authentication mode. Exactly one mode is active at a time.
pub enum AuthMode {
    /// Token-based auth (api_key or oidc) — uses Bearer token from Authorization header.
    Token(Arc<dyn AuthProvider>),
    /// Trusted headers auth — reads user identity from proxy-set headers.
    TrustedHeaders(Arc<TrustedHeadersAuthProvider>),
}

/// Validate that `senko serve` has exactly one authentication method configured.
/// Call before `create_auth_mode`.
pub fn validate_serve_auth(config: &Config) -> Result<()> {
    if !config.server.auth.is_configured() {
        bail!(
            "senko serve requires an authentication method. \
             Set server.auth.oidc (issuer_url + client_id), \
             server.auth.api_key.master_key, or \
             server.auth.trusted_headers.subject_header."
        );
    }
    config
        .server
        .auth
        .validate_exclusive()
        .map_err(|msg| anyhow::anyhow!(msg))?;
    Ok(())
}

pub fn create_auth_mode(
    config: &Config,
    backend: Arc<dyn TaskBackend>,
) -> Result<Option<AuthMode>> {
    let auth = &config.server.auth;

    if auth.oidc.is_configured() {
        let issuer_url = auth.oidc.issuer_url.clone().unwrap();
        let client_id = auth.oidc.client_id.clone().unwrap();
        let username_claim = auth.oidc.username_claim.clone();
        let required_claims = auth.oidc.required_claims.clone();
        let groups_claim = auth.oidc.groups_claim.clone();
        let master_group = auth.oidc.master_group.clone();
        tracing::info!(issuer = %issuer_url, "OIDC JWT authentication enabled");
        let user_ops: Arc<dyn crate::application::UserOperations> =
            Arc::new(UserService::new(backend.clone()));
        return Ok(Some(AuthMode::Token(Arc::new(JwtAuthProvider::new(
            issuer_url,
            client_id,
            username_claim,
            required_claims,
            groups_claim,
            master_group,
            user_ops,
        )))));
    }

    if auth.api_key.master_key.is_some() {
        tracing::info!("API key authentication enabled");
        return Ok(Some(AuthMode::Token(Arc::new(ApiKeyProvider::new(
            backend,
            auth.api_key.master_key.clone(),
            auth.oidc.session.clone(),
        )))));
    }

    if auth.trusted_headers.is_configured() {
        let subject_header = auth.trusted_headers.subject_header.clone().unwrap();
        tracing::info!(header = %subject_header, "trusted headers authentication enabled");
        let user_ops: Arc<dyn crate::application::UserOperations> =
            Arc::new(UserService::new(backend));
        return Ok(Some(AuthMode::TrustedHeaders(Arc::new(
            TrustedHeadersAuthProvider::new(
                user_ops,
                subject_header,
                auth.trusted_headers.name_header.clone(),
                auth.trusted_headers.display_name_header.clone(),
                auth.trusted_headers.email_header.clone(),
                auth.trusted_headers.groups_header.clone(),
                auth.trusted_headers.scope_header.clone(),
                auth.trusted_headers.master_group.clone(),
            ),
        ))));
    }

    tracing::info!("no authentication method configured");
    Ok(None)
}

pub fn create_local_task_operations(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    project_root: &Path,
) -> LocalTaskOperations {
    let backend_info = resolve_backend_info(config, project_root);
    let hook_data: Arc<dyn HookDataSource> =
        Arc::new(crate::application::port::BackendHookData(backend.clone()));
    let hooks = create_hook_executor(config.clone(), RuntimeMode::Cli, backend_info, hook_data);
    let pr_verifier: Arc<dyn PrVerifier> = Arc::new(GhCliPrVerifier);
    let completion_policy = CompletionPolicy::new(config.workflow.merge_via);
    LocalTaskOperations::new(backend, hooks, pr_verifier, completion_policy)
}

pub fn create_remote_task_operations(
    config: &Config,
    project_root: &Path,
    cli_attrs: &[(String, String)],
) -> RemoteTaskOperations {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    let attributes = resolve_trace_attributes(cli_attrs);

    let hook_data: Arc<dyn HookDataSource> = Arc::new(RemoteHookDataSource::new(
        url,
        api_key.clone(),
        attributes.clone(),
    ));
    let backend_info = resolve_backend_info(config, project_root);
    let hooks = create_hook_executor(config.clone(), RuntimeMode::Cli, backend_info, hook_data);

    RemoteTaskOperations::new(url, api_key, attributes, hooks)
}

/// Create the appropriate `TaskOperations` and `ProjectOperations` based on config.
///
/// Remote mode uses HTTP-based Remote*Operations; local mode uses DB-backed services.
pub fn create_task_operations(
    project_root: &Path,
    config: &Config,
    cli_attrs: &[(String, String)],
) -> Result<(Arc<dyn TaskOperations>, Arc<dyn ProjectOperations>)> {
    if config.cli.remote.url.is_some() {
        let task_ops: Arc<dyn TaskOperations> = Arc::new(create_remote_task_operations(
            config,
            project_root,
            cli_attrs,
        ));
        let project_ops: Arc<dyn ProjectOperations> =
            Arc::new(create_remote_project_operations(config, cli_attrs));
        Ok((task_ops, project_ops))
    } else {
        let backend = create_backend(project_root, config)?;
        let task_ops: Arc<dyn TaskOperations> = Arc::new(create_local_task_operations(
            backend.clone(),
            config,
            project_root,
        ));
        let project_ops: Arc<dyn ProjectOperations> = Arc::new(ProjectService::new(backend));
        Ok((task_ops, project_ops))
    }
}

pub fn create_project_service(backend: Arc<dyn TaskBackend>) -> ProjectService {
    ProjectService::new(backend)
}

pub fn create_user_service(backend: Arc<dyn TaskBackend>) -> UserService {
    UserService::new(backend)
}

pub fn create_remote_user_operations(
    config: &Config,
    cli_attrs: &[(String, String)],
) -> RemoteUserOperations {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    RemoteUserOperations::new(url, api_key, resolve_trace_attributes(cli_attrs))
}

pub fn create_remote_project_operations(
    config: &Config,
    cli_attrs: &[(String, String)],
) -> RemoteProjectOperations {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    RemoteProjectOperations::new(url, api_key, resolve_trace_attributes(cli_attrs))
}

pub fn create_metadata_field_service(backend: Arc<dyn TaskBackend>) -> MetadataFieldService {
    MetadataFieldService::new(backend)
}

/// Create a `LocalContractOperations` wired up with a hook executor for the
/// given runtime. Use this when callers know the runtime they are executing in
/// (CLI direct, server direct, etc.).
pub fn create_local_contract_operations(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    project_root: &Path,
    runtime_mode: RuntimeMode,
) -> LocalContractOperations {
    let backend_info = resolve_backend_info(config, project_root);
    let hook_data: Arc<dyn HookDataSource> =
        Arc::new(crate::application::port::BackendHookData(backend.clone()));
    let hooks = create_hook_executor(config.clone(), runtime_mode, backend_info, hook_data);
    LocalContractOperations::new(backend, hooks)
}

/// Backwards-compatible helper used by CLI paths (always CLI runtime).
pub fn create_contract_service(
    backend: Arc<dyn TaskBackend>,
    config: &Config,
    project_root: &Path,
) -> LocalContractOperations {
    create_local_contract_operations(backend, config, project_root, RuntimeMode::Cli)
}

pub fn create_remote_contract_operations(
    config: &Config,
    project_root: &Path,
    cli_attrs: &[(String, String)],
) -> RemoteContractOperations {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    let attributes = resolve_trace_attributes(cli_attrs);

    let hook_data: Arc<dyn HookDataSource> = Arc::new(RemoteHookDataSource::new(
        url,
        api_key.clone(),
        attributes.clone(),
    ));
    let backend_info = resolve_backend_info(config, project_root);
    let hooks = create_hook_executor(config.clone(), RuntimeMode::Cli, backend_info, hook_data);

    RemoteContractOperations::new(url, api_key, attributes, hooks)
}

pub fn create_remote_hook_data(
    config: &Config,
    cli_attrs: &[(String, String)],
) -> Arc<dyn HookDataSource> {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    Arc::new(RemoteHookDataSource::new(
        url,
        api_key,
        resolve_trace_attributes(cli_attrs),
    ))
}

pub fn create_hook_data_from(
    url: &str,
    token: Option<String>,
    cli_attrs: &[(String, String)],
) -> Arc<dyn HookDataSource> {
    Arc::new(RemoteHookDataSource::new(
        url,
        token,
        resolve_trace_attributes(cli_attrs),
    ))
}

pub fn create_remote_metadata_field_operations(
    config: &Config,
    cli_attrs: &[(String, String)],
) -> RemoteMetadataFieldOperations {
    let url = config
        .cli
        .remote
        .url
        .as_ref()
        .expect("cli.remote.url required for remote operations");
    let api_key = config.cli.remote.token.clone();
    RemoteMetadataFieldOperations::new(url, api_key, resolve_trace_attributes(cli_attrs))
}

pub fn create_hook_test_service(
    hook_data: Arc<dyn HookDataSource>,
    config: &Config,
    project_root: &Path,
) -> HookTestService {
    let backend_info = resolve_backend_info(config, project_root);
    let hook_test = Arc::new(ShellHookTestExecutor::new(
        config.clone(),
        RuntimeMode::Cli,
        backend_info,
        hook_data.clone(),
    ));
    HookTestService::new(hook_data, hook_test)
}

/// Resolve the project ID from config (CLI > env > config.toml already applied).
pub async fn resolve_project_id(
    project_ops: &dyn ProjectOperations,
    config: &Config,
) -> Result<ProjectId> {
    match config.project.name.as_deref() {
        Some(n) => {
            let project = project_ops
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
    user_ops: &dyn crate::application::UserOperations,
    config: &Config,
) -> Result<crate::domain::user::UserId> {
    match config.user.name.as_deref() {
        Some(n) => {
            let username = crate::domain::user::Username::try_from(n.to_string())
                .with_context(|| format!("invalid user name in config: {n}"))?;
            let user = user_ops
                .get_user_by_username(&username)
                .await
                .with_context(|| format!("user not found: {n}"))?;
            Ok(user.id())
        }
        None => Ok(DEFAULT_USER_ID),
    }
}

pub fn init_tracing(config: &LogConfig) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

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

/// Guard returned by [`init_telemetry`] holding the OTel providers, so the
/// server can flush pending spans / logs during graceful shutdown. `None` when
/// an exporter is disabled (`OTEL_*_EXPORTER=none`, `OTEL_SDK_DISABLED=true`,
/// or build failure).
pub struct TelemetryGuard {
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
}

impl TelemetryGuard {
    /// Flush and shut down both providers. Errors are demoted to `warn!` —
    /// shutdown must never panic or propagate.
    pub fn shutdown(self) {
        if let Some(tp) = self.tracer_provider
            && let Err(e) = tp.shutdown()
        {
            tracing::warn!(error = %e, "tracer provider shutdown failed");
        }
        if let Some(lp) = self.logger_provider
            && let Err(e) = lp.shutdown()
        {
            tracing::warn!(error = %e, "logger provider shutdown failed");
        }
    }
}

/// Build the OTel `Resource` used by both the tracer and logger providers.
///
/// Resource attribute policy:
/// - `service.name`: pinned to `"senko-server"` (overrides `OTEL_SERVICE_NAME`
///   and `service.name` in `OTEL_RESOURCE_ATTRIBUTES` — pre-existing behavior).
/// - `service.version`: defaults to `CARGO_PKG_VERSION`; env-supplied
///   `OTEL_RESOURCE_ATTRIBUTES=service.version=...` takes precedence.
/// - `senko.version`: always `CARGO_PKG_VERSION`. Cannot be overridden by
///   env — operators should treat this as an authoritative provenance tag.
pub(crate) fn build_telemetry_resource() -> opentelemetry_sdk::Resource {
    use opentelemetry::KeyValue;
    use opentelemetry_sdk::Resource;

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let env_has_service_version = std::env::var("OTEL_RESOURCE_ATTRIBUTES")
        .ok()
        .map(|s| {
            parse_otel_resource_attributes(&s)
                .iter()
                .any(|(k, _)| k == "service.version")
        })
        .unwrap_or(false);

    let mut builder = Resource::builder().with_service_name("senko-server");
    if !env_has_service_version {
        builder = builder.with_attribute(KeyValue::new("service.version", VERSION));
    }
    builder
        .with_attribute(KeyValue::new("senko.version", VERSION))
        .build()
}

/// Initialize tracing + OTel SDK for `senko serve` / `senko serve --proxy`.
///
/// Behavior is controlled entirely by OTel standard environment variables;
/// senko adds no TOML knobs for telemetry per Contract #7:
///
/// - `OTEL_SDK_DISABLED=true` — short-circuits every OTel layer. The base
///   `tracing_subscriber::fmt` logger still runs so local debugging works.
/// - `OTEL_TRACES_EXPORTER` (default `otlp`): `otlp`, `console`, `none`.
/// - `OTEL_LOGS_EXPORTER` (default `otlp`): same values.
/// - `OTEL_EXPORTER_OTLP_PROTOCOL` (default `grpc`): `grpc` or `http/protobuf`.
///   Per-signal overrides `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL` /
///   `OTEL_EXPORTER_OTLP_LOGS_PROTOCOL` take precedence over the global value.
///   Senko intentionally keeps `grpc` as the default for back-compat (the OTel
///   spec default of `http/protobuf` is NOT followed). `http/json` is not
///   supported; any other value logs a warning and disables that signal.
/// - `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`,
///   `OTEL_RESOURCE_ATTRIBUTES` — read by the OTel SDK directly.
///
/// Unknown exporter values log a warning and behave like `none`.
pub fn init_telemetry(config: &LogConfig) -> TelemetryGuard {
    use opentelemetry::propagation::TextMapCompositePropagator;
    use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};

    // Always install the composite propagator so the HTTP middleware can
    // extract `traceparent` + `baggage` regardless of whether we're exporting.
    opentelemetry::global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));
    let fmt_layer: Box<dyn tracing_subscriber::Layer<_> + Send + Sync> = match config.format {
        LogFormat::Json => Box::new(tracing_subscriber::fmt::layer().json()),
        LogFormat::Pretty => Box::new(tracing_subscriber::fmt::layer()),
    };

    if std::env::var("OTEL_SDK_DISABLED").ok().as_deref() == Some("true") {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
        tracing::info!("OTEL_SDK_DISABLED=true — OTel exporters skipped");
        return TelemetryGuard {
            tracer_provider: None,
            logger_provider: None,
        };
    }

    let resource = build_telemetry_resource();

    let tracer_provider = build_tracer_provider(resource.clone());
    let logger_provider = build_logger_provider(resource);

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    // Attach the OTel span layer only when we actually have a tracer provider.
    // `tracing_opentelemetry::layer()` without `with_tracer` installs a noop
    // and costs nothing, but installing a proper tracer lets us skip the layer
    // entirely in `none` mode — cheaper spans on a hot path.
    match (&tracer_provider, &logger_provider) {
        (Some(tp), Some(lp)) => {
            let tracer = opentelemetry::trace::TracerProvider::tracer(tp, "senko-server");
            registry
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(lp))
                .init();
            tracing::info!("OTel telemetry initialized with traces + logs exporters");
        }
        (Some(tp), None) => {
            let tracer = opentelemetry::trace::TracerProvider::tracer(tp, "senko-server");
            registry
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .init();
            tracing::info!("OTel telemetry initialized with traces exporter only");
        }
        (None, Some(lp)) => {
            registry
                .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(lp))
                .init();
            tracing::info!("OTel telemetry initialized with logs exporter only");
        }
        (None, None) => {
            registry.init();
            tracing::info!("OTel telemetry initialized without exporters");
        }
    }

    if let Some(ref tp) = tracer_provider {
        opentelemetry::global::set_tracer_provider(tp.clone());
    }
    // `opentelemetry::global` exposes no logger-provider setter in 0.31; the
    // tracing bridge receives the provider by reference, so no global install
    // is needed for log flow. The guard still owns `logger_provider` so we can
    // flush it on shutdown.

    TelemetryGuard {
        tracer_provider,
        logger_provider,
    }
}

/// Resolve the effective exporter choice for traces/logs.
///
/// Per Contract #7 ("未設定時は export しない") the default is **not** the
/// OTel-spec default of `"otlp"`: a bare `senko serve` with no OTel env at
/// all must emit nothing, to avoid log spam / connect retries against a
/// non-existent collector in local-dev. The user opts in by either setting
/// `<OTEL_*_EXPORTER>` explicitly (honor whatever they chose, including
/// `otlp` / `console` / `none`) OR by setting `OTEL_EXPORTER_OTLP_ENDPOINT`
/// (signals intent to talk to a collector → use `otlp`).
fn resolve_exporter_choice(exporter_env: &str) -> String {
    if let Ok(v) = std::env::var(exporter_env)
        && !v.is_empty()
    {
        return v;
    }
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        return "otlp".to_string();
    }
    "none".to_string()
}

/// OTLP transport protocol senko supports. `http/json` is intentionally
/// excluded — see [`init_telemetry`] doc and task #370 DoD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OtlpExportProtocol {
    Grpc,
    HttpProtobuf,
}

/// Resolve the OTLP transport protocol for a given signal.
///
/// Lookup order (first non-empty wins):
/// 1. `signal_env` (e.g. `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL`)
/// 2. `OTEL_EXPORTER_OTLP_PROTOCOL`
/// 3. default → [`OtlpExportProtocol::Grpc`]
///
/// Senko keeps `Grpc` as the default for back-compat with 0.38.1 and earlier;
/// the OTel-spec default of `http/protobuf` is intentionally NOT followed.
/// `Err(raw)` carries the unrecognized value so the caller can warn-and-disable
/// the signal — there is no silent fallback to gRPC.
fn resolve_otlp_protocol(signal_env: &str) -> Result<OtlpExportProtocol, String> {
    let raw = std::env::var(signal_env)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL")
                .ok()
                .filter(|v| !v.is_empty())
        });
    match raw.as_deref() {
        None => Ok(OtlpExportProtocol::Grpc),
        Some("grpc") => Ok(OtlpExportProtocol::Grpc),
        Some("http/protobuf") => Ok(OtlpExportProtocol::HttpProtobuf),
        Some(other) => Err(other.to_string()),
    }
}

fn build_tracer_provider(
    resource: opentelemetry_sdk::Resource,
) -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry_otlp::WithExportConfig;

    let choice = resolve_exporter_choice("OTEL_TRACES_EXPORTER");
    match choice.as_str() {
        "otlp" => {
            let protocol = match resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL") {
                Ok(p) => p,
                Err(raw) => {
                    tracing::warn!(
                        value = %raw,
                        "unknown OTEL_EXPORTER_OTLP_(TRACES_)PROTOCOL value; traces disabled",
                    );
                    return None;
                }
            };
            let builder = opentelemetry_otlp::SpanExporter::builder();
            let result = match protocol {
                OtlpExportProtocol::Grpc => builder.with_tonic().build(),
                OtlpExportProtocol::HttpProtobuf => builder
                    .with_http()
                    .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
                    .build(),
            };
            match result {
                Ok(exporter) => Some(
                    opentelemetry_sdk::trace::SdkTracerProvider::builder()
                        .with_batch_exporter(exporter)
                        .with_resource(resource)
                        .build(),
                ),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to build OTLP span exporter; traces disabled");
                    None
                }
            }
        }
        "console" => Some(
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
                .with_resource(resource)
                .build(),
        ),
        "none" => None,
        other => {
            tracing::warn!(
                value = %other,
                "unknown OTEL_TRACES_EXPORTER value; traces disabled",
            );
            None
        }
    }
}

fn build_logger_provider(
    resource: opentelemetry_sdk::Resource,
) -> Option<opentelemetry_sdk::logs::SdkLoggerProvider> {
    use opentelemetry_otlp::WithExportConfig;

    let choice = resolve_exporter_choice("OTEL_LOGS_EXPORTER");
    match choice.as_str() {
        "otlp" => {
            let protocol = match resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL") {
                Ok(p) => p,
                Err(raw) => {
                    tracing::warn!(
                        value = %raw,
                        "unknown OTEL_EXPORTER_OTLP_(LOGS_)PROTOCOL value; OTel logs disabled",
                    );
                    return None;
                }
            };
            let builder = opentelemetry_otlp::LogExporter::builder();
            let result = match protocol {
                OtlpExportProtocol::Grpc => builder.with_tonic().build(),
                OtlpExportProtocol::HttpProtobuf => builder
                    .with_http()
                    .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
                    .build(),
            };
            match result {
                Ok(exporter) => Some(
                    opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                        .with_log_processor(
                            crate::application::telemetry::BusinessAttributesProcessor,
                        )
                        .with_batch_exporter(exporter)
                        .with_resource(resource)
                        .build(),
                ),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to build OTLP log exporter; OTel logs disabled");
                    None
                }
            }
        }
        "console" => Some(
            opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                .with_log_processor(crate::application::telemetry::BusinessAttributesProcessor)
                .with_simple_exporter(opentelemetry_stdout::LogExporter::default())
                .with_resource(resource)
                .build(),
        ),
        "none" => None,
        other => {
            tracing::warn!(
                value = %other,
                "unknown OTEL_LOGS_EXPORTER value; OTel logs disabled",
            );
            None
        }
    }
}

pub fn load_config(
    project_root: &Path,
    explicit_config: Option<&Path>,
    xdg: &XdgDirs,
) -> Result<Config> {
    // 1. Load user config + user local overlay
    let (user_raw, user_local) = load_user_config(xdg)?;

    // 2. Load project/explicit config + its local overlay
    let (project_raw, project_local) = if let Some(path) = explicit_config {
        let raw = load_config_file(path, true)?;
        let local = load_local_overlay(path)?;
        (Some(raw), local)
    } else if let Some(env_path) = env_config_path() {
        let raw = load_config_file(&env_path, true)?;
        let local = load_local_overlay(&env_path)?;
        (Some(raw), local)
    } else {
        let default_path = project_root.join(".senko").join("config.toml");
        if default_path.exists() {
            let raw = load_config_file(&default_path, false)?;
            let local = load_local_overlay(&default_path)?;
            (Some(raw), local)
        } else {
            (None, None)
        }
    };

    // 3. Merge: user → user local → project → project local
    let mut merged = user_raw.unwrap_or_default();
    if let Some(local) = user_local {
        merged = merged.merge(local);
    }
    if let Some(project) = project_raw {
        merged = merged.merge(project);
    }
    if let Some(local) = project_local {
        merged = merged.merge(local);
    }

    // 4. Resolve to final Config and apply env overrides
    let mut config = merged.resolve();
    config.apply_env();
    config.xdg = xdg.clone();
    Ok(config)
}

/// Return the user-level config path.
/// `$XDG_CONFIG_HOME/senko/config.toml` or `~/.config/senko/config.toml`
fn user_config_path(xdg: &XdgDirs) -> Option<PathBuf> {
    xdg.config_home
        .as_ref()
        .map(|dir| dir.join("senko").join("config.toml"))
}

/// Load user-level config and its local overlay if they exist.
fn load_user_config(xdg: &XdgDirs) -> Result<(Option<RawConfig>, Option<RawConfig>)> {
    let path = match user_config_path(xdg) {
        Some(p) if p.exists() => p,
        _ => return Ok((None, None)),
    };
    let raw = load_config_file(&path, false)?;
    let local = load_local_overlay(&path)?;
    Ok((Some(raw), local))
}

/// Load config.local.toml from the same directory as the given config file.
fn load_local_overlay(config_path: &Path) -> Result<Option<RawConfig>> {
    let local_path = config_path.with_file_name("config.local.toml");
    if local_path.exists() {
        Ok(Some(load_config_file(&local_path, false)?))
    } else {
        Ok(None)
    }
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

/// Check if the config still uses the legacy `[hooks]` format.
/// Emits a warning on any `[hooks]` presence (since the top-level section is
/// no longer honored) and returns an error only when the legacy string / array
/// shape is used, which would previously have caused a silent parse error.
fn detect_legacy_hook_format(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Ok(()), // let the real parser produce the error
    };
    let Some(hooks_table) = raw.get("hooks").and_then(|v| v.as_table()) else {
        return Ok(());
    };

    // Legacy scalar/array format (e.g., `[hooks]` on_task_added = "cmd"`).
    for (key, val) in hooks_table {
        if val.is_str() || val.is_array() {
            bail!(
                "Legacy hook format detected in {}.\n\
                 The array-based hook format is no longer supported.\n\
                 Migrate to the runtime-scoped schema:\n\n\
                 Old format:\n  [hooks]\n  {} = \"command\"\n\n\
                 New format:\n  [cli.{}.hooks.my-hook]\n  command = \"command\"\n",
                path.display(),
                key,
                key,
            );
        }
    }

    // Nested legacy schema ([hooks.on_task_*.name]). Parseable, but silently
    // ignored under the new schema — warn the user to migrate.
    tracing::warn!(
        path = %path.display(),
        "`[hooks]` section found in config is no longer honored; migrate hook definitions to \
         [cli.<action>.hooks.<name>] / [server.relay.<action>.hooks.<name>] / \
         [server.remote.<action>.hooks.<name>] / [workflow.<stage>.hooks.<name>]"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;

    /// Build an isolated `XdgDirs` that points config_home at an empty directory
    /// under `project_root`, so no user-level config file is ever found.
    fn isolated_xdg(project_root: &Path) -> XdgDirs {
        XdgDirs {
            config_home: Some(project_root.join("__no_user_config__")),
            ..Default::default()
        }
    }

    /// Clear SENKO_* env vars that still feed into `load_config` (not XDG).
    /// Kept as a helper because `SENKO_CONFIG` / `SENKO_USER` / `SENKO_PROJECT`
    /// are read via `std::env::var` from inside `env_config_path`, so tests
    /// that must be isolated from the real environment still need this.
    fn clear_senko_env() {
        // SAFETY: callers are marked #[serial].
        unsafe {
            std::env::remove_var("SENKO_CONFIG");
            std::env::remove_var("SENKO_USER");
            std::env::remove_var("SENKO_PROJECT");
        }
    }

    /// Run `load_config` in an isolated environment where no real user config
    /// or env-var config can leak in.
    fn load_config_isolated(project_root: &Path) -> Result<Config> {
        clear_senko_env();
        load_config(project_root, None, &isolated_xdg(project_root))
    }

    #[test]
    #[serial]
    fn load_config_with_local_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let senko_dir = dir.path().join(".senko");
        fs::create_dir_all(&senko_dir).unwrap();

        fs::write(
            senko_dir.join("config.toml"),
            r#"
[user]
name = "project-user"

[project]
name = "my-project"
"#,
        )
        .unwrap();

        fs::write(
            senko_dir.join("config.local.toml"),
            r#"
[user]
name = "local-user"
"#,
        )
        .unwrap();

        let config = load_config_isolated(dir.path()).unwrap();
        assert_eq!(config.user.name.as_deref(), Some("local-user"));
        assert_eq!(config.project.name.as_deref(), Some("my-project"));
    }

    #[test]
    #[serial]
    fn load_config_without_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let senko_dir = dir.path().join(".senko");
        fs::create_dir_all(&senko_dir).unwrap();

        fs::write(
            senko_dir.join("config.toml"),
            r#"
[user]
name = "project-user"
"#,
        )
        .unwrap();

        let config = load_config_isolated(dir.path()).unwrap();
        assert_eq!(config.user.name.as_deref(), Some("project-user"));
    }

    #[test]
    #[serial]
    fn load_config_explicit_config_uses_sibling_local() {
        let dir = tempfile::tempdir().unwrap();
        let custom_dir = dir.path().join("custom");
        fs::create_dir_all(&custom_dir).unwrap();

        fs::write(
            custom_dir.join("config.toml"),
            r#"
[user]
name = "custom-user"
"#,
        )
        .unwrap();

        fs::write(
            custom_dir.join("config.local.toml"),
            r#"
[user]
name = "custom-local-user"
"#,
        )
        .unwrap();

        clear_senko_env();
        let xdg = isolated_xdg(dir.path());
        let config = load_config(dir.path(), Some(&custom_dir.join("config.toml")), &xdg).unwrap();
        assert_eq!(config.user.name.as_deref(), Some("custom-local-user"));
    }

    #[test]
    #[serial]
    fn load_config_explicit_config_ignores_project_local() {
        let dir = tempfile::tempdir().unwrap();
        let senko_dir = dir.path().join(".senko");
        let custom_dir = dir.path().join("custom");
        fs::create_dir_all(&senko_dir).unwrap();
        fs::create_dir_all(&custom_dir).unwrap();

        // Project local overlay should NOT be loaded when --config is used
        fs::write(
            senko_dir.join("config.local.toml"),
            r#"
[user]
name = "project-local-user"
"#,
        )
        .unwrap();

        fs::write(
            custom_dir.join("config.toml"),
            r#"
[user]
name = "custom-user"
"#,
        )
        .unwrap();

        clear_senko_env();
        let xdg = isolated_xdg(dir.path());
        let config = load_config(dir.path(), Some(&custom_dir.join("config.toml")), &xdg).unwrap();
        // Should be "custom-user", NOT "project-local-user"
        assert_eq!(config.user.name.as_deref(), Some("custom-user"));
    }

    #[test]
    #[serial]
    fn load_config_user_local_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let user_config_dir = dir.path().join("user_config").join("senko");
        fs::create_dir_all(&user_config_dir).unwrap();

        fs::write(
            user_config_dir.join("config.toml"),
            r#"
[user]
name = "base-user"
"#,
        )
        .unwrap();

        fs::write(
            user_config_dir.join("config.local.toml"),
            r#"
[user]
name = "user-local-override"
"#,
        )
        .unwrap();

        // Point config_home at our test dir via injected XdgDirs (no env mutation).
        clear_senko_env();
        let xdg = XdgDirs {
            config_home: Some(dir.path().join("user_config")),
            ..Default::default()
        };

        let project_dir = dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let config = load_config(&project_dir, None, &xdg).unwrap();
        assert_eq!(config.user.name.as_deref(), Some("user-local-override"));
    }

    #[test]
    #[serial]
    fn load_config_merge_order() {
        // Verify: user → user local → project → project local
        let dir = tempfile::tempdir().unwrap();
        let user_config_dir = dir.path().join("user_config").join("senko");
        let senko_dir = dir.path().join("project").join(".senko");
        fs::create_dir_all(&user_config_dir).unwrap();
        fs::create_dir_all(&senko_dir).unwrap();

        // User config sets user.name and project.name
        fs::write(
            user_config_dir.join("config.toml"),
            r#"
[user]
name = "user-base"

[project]
name = "user-project"
"#,
        )
        .unwrap();

        // User local overrides user.name only
        fs::write(
            user_config_dir.join("config.local.toml"),
            r#"
[user]
name = "user-local"
"#,
        )
        .unwrap();

        // Project config overrides project.name, sets a new field
        fs::write(
            senko_dir.join("config.toml"),
            r#"
[project]
name = "project-base"
"#,
        )
        .unwrap();

        // Project local overrides project.name
        fs::write(
            senko_dir.join("config.local.toml"),
            r#"
[project]
name = "project-local"
"#,
        )
        .unwrap();

        clear_senko_env();
        let xdg = XdgDirs {
            config_home: Some(dir.path().join("user_config")),
            ..Default::default()
        };

        let project_dir = dir.path().join("project");
        let config = load_config(&project_dir, None, &xdg).unwrap();

        // user.name: user-base → user-local (user local wins over user base)
        // project config and project local don't set user.name, so user-local stays
        assert_eq!(config.user.name.as_deref(), Some("user-local"));

        // project.name: user-project → (user local doesn't set it) → project-base → project-local
        assert_eq!(config.project.name.as_deref(), Some("project-local"));
    }

    // --- auth config validation tests ---

    #[test]
    fn validate_serve_auth_with_oidc_ok() {
        let mut config = Config::default();
        config.server.auth.oidc.issuer_url = Some("https://example.com".to_string());
        config.server.auth.oidc.client_id = Some("my-client".to_string());
        validate_serve_auth(&config).unwrap();
    }

    #[test]
    fn validate_serve_auth_with_master_key_ok() {
        let mut config = Config::default();
        config.server.auth.api_key.master_key = Some("secret".to_string());
        validate_serve_auth(&config).unwrap();
    }

    #[test]
    fn validate_serve_auth_with_trusted_headers_ok() {
        let mut config = Config::default();
        config.server.auth.trusted_headers.subject_header = Some("x-senko-user-sub".to_string());
        validate_serve_auth(&config).unwrap();
    }

    #[test]
    fn validate_serve_auth_with_oidc_and_api_key_fails() {
        let mut config = Config::default();
        config.server.auth.oidc.issuer_url = Some("https://example.com".to_string());
        config.server.auth.oidc.client_id = Some("my-client".to_string());
        config.server.auth.api_key.master_key = Some("secret".to_string());
        let err = validate_serve_auth(&config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("only one authentication mode"),
            "error should mention exclusivity: {msg}"
        );
    }

    #[test]
    fn validate_serve_auth_with_oidc_and_trusted_headers_fails() {
        let mut config = Config::default();
        config.server.auth.oidc.issuer_url = Some("https://example.com".to_string());
        config.server.auth.oidc.client_id = Some("my-client".to_string());
        config.server.auth.trusted_headers.subject_header = Some("x-senko-user-sub".to_string());
        let err = validate_serve_auth(&config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("only one authentication mode"),
            "error should mention exclusivity: {msg}"
        );
    }

    #[test]
    fn validate_serve_auth_with_all_three_fails() {
        let mut config = Config::default();
        config.server.auth.oidc.issuer_url = Some("https://example.com".to_string());
        config.server.auth.oidc.client_id = Some("my-client".to_string());
        config.server.auth.api_key.master_key = Some("secret".to_string());
        config.server.auth.trusted_headers.subject_header = Some("x-senko-user-sub".to_string());
        let err = validate_serve_auth(&config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("only one authentication mode"),
            "error should mention exclusivity: {msg}"
        );
    }

    #[test]
    fn validate_serve_auth_with_neither_fails() {
        let config = Config::default();
        let err = validate_serve_auth(&config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("api_key.master_key"),
            "error should mention api_key.master_key: {msg}"
        );
        assert!(msg.contains("oidc"), "error should mention oidc: {msg}");
        assert!(
            msg.contains("trusted_headers"),
            "error should mention trusted_headers: {msg}"
        );
    }

    // --- resolve_trace_attributes ---

    /// Clear all trace-related env vars. Callers MUST be `#[serial]`.
    fn clear_trace_env() {
        // SAFETY: callers are marked #[serial].
        unsafe {
            std::env::remove_var("SENKO_TRACE_ATTRIBUTES");
            std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES");
        }
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_contains_only_auto_when_no_sources() {
        clear_trace_env();
        let got = resolve_trace_attributes(&[]);
        // Always auto-populated with senko.operation.id.
        assert_eq!(got.len(), 1, "unexpected extra keys: {got:?}");
        let id = got
            .get("senko.operation.id")
            .expect("senko.operation.id must be present");
        Uuid::parse_str(id).expect("senko.operation.id must be a UUID");
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_cli_wins_over_senko_env() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("SENKO_TRACE_ATTRIBUTES", "run.id=from-senko");
        }
        let got = resolve_trace_attributes(&[pair("run.id", "from-cli")]);
        assert_eq!(got.get("run.id"), Some(&"from-cli".to_string()));
        clear_trace_env();
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_senko_env_wins_over_otel_env() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("SENKO_TRACE_ATTRIBUTES", "run.id=from-senko");
            std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "run.id=from-otel");
        }
        let got = resolve_trace_attributes(&[]);
        assert_eq!(got.get("run.id"), Some(&"from-senko".to_string()));
        clear_trace_env();
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_filters_reserved_from_otel_env_only() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var(
                "OTEL_RESOURCE_ATTRIBUTES",
                "service.name=senko-cli,run.id=visible",
            );
            std::env::set_var("SENKO_TRACE_ATTRIBUTES", "service.name=from-senko");
        }
        let got = resolve_trace_attributes(&[pair("host.name", "from-cli")]);
        // service.name from OTEL is filtered, but SENKO value survives.
        assert_eq!(got.get("service.name"), Some(&"from-senko".to_string()));
        // host.name from CLI is NOT filtered (explicit user override).
        assert_eq!(got.get("host.name"), Some(&"from-cli".to_string()));
        // Non-reserved key from OTEL passes through.
        assert_eq!(got.get("run.id"), Some(&"visible".to_string()));
        clear_trace_env();
    }

    // --- auto senko.operation.id ---

    #[test]
    fn auto_operation_id_stable_across_calls() {
        // OnceLock caches the first value — a single process must always
        // observe the same ID regardless of how many times it's resolved.
        let a = auto_operation_id().to_string();
        let b = auto_operation_id().to_string();
        assert_eq!(a, b);
    }

    #[test]
    fn auto_operation_id_is_valid_uuid() {
        let id = auto_operation_id();
        let parsed = Uuid::parse_str(id).expect("auto_operation_id must be a valid UUID");
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_auto_operation_id_present_when_no_sources() {
        clear_trace_env();
        let got = resolve_trace_attributes(&[]);
        let id = got
            .get("senko.operation.id")
            .expect("senko.operation.id must be auto-populated");
        assert_eq!(id, auto_operation_id());
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_cli_wins_over_auto_operation_id() {
        clear_trace_env();
        let got = resolve_trace_attributes(&[pair("senko.operation.id", "custom-cli")]);
        assert_eq!(
            got.get("senko.operation.id"),
            Some(&"custom-cli".to_string()),
        );
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_senko_env_wins_over_auto_operation_id() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("SENKO_TRACE_ATTRIBUTES", "senko.operation.id=env-val");
        }
        let got = resolve_trace_attributes(&[]);
        assert_eq!(got.get("senko.operation.id"), Some(&"env-val".to_string()),);
        clear_trace_env();
    }

    #[test]
    #[serial]
    fn resolve_trace_attributes_otel_env_wins_over_auto_operation_id() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "senko.operation.id=otel-val");
        }
        let got = resolve_trace_attributes(&[]);
        assert_eq!(got.get("senko.operation.id"), Some(&"otel-val".to_string()),);
        clear_trace_env();
    }

    // --- build_telemetry_resource ---

    /// Test-only exporter that captures the `Resource` forwarded by the SDK
    /// via `set_resource`. We use this instead of `InMemorySpanExporter`
    /// because the latter stores the resource privately with no getter.
    #[derive(Clone, Debug, Default)]
    struct ResourceCapturingExporter {
        resource: std::sync::Arc<std::sync::Mutex<Option<opentelemetry_sdk::Resource>>>,
    }

    impl ResourceCapturingExporter {
        fn captured(&self) -> opentelemetry_sdk::Resource {
            self.resource
                .lock()
                .unwrap()
                .clone()
                .expect("set_resource was not called")
        }
    }

    impl opentelemetry_sdk::trace::SpanExporter for ResourceCapturingExporter {
        async fn export(
            &self,
            _batch: Vec<opentelemetry_sdk::trace::SpanData>,
        ) -> opentelemetry_sdk::error::OTelSdkResult {
            Ok(())
        }

        fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
            *self.resource.lock().unwrap() = Some(resource.clone());
        }
    }

    /// Build a provider with our resource and return what the exporter received.
    fn capture_resource() -> opentelemetry_sdk::Resource {
        use opentelemetry_sdk::trace::SdkTracerProvider;
        let exporter = ResourceCapturingExporter::default();
        // `build()` calls `set_resource` on every processor synchronously,
        // which forwards to the exporter — no span needs to be emitted.
        let _provider = SdkTracerProvider::builder()
            .with_resource(build_telemetry_resource())
            .with_simple_exporter(exporter.clone())
            .build();
        exporter.captured()
    }

    fn attr(resource: &opentelemetry_sdk::Resource, key: &str) -> Option<String> {
        resource
            .get(&opentelemetry::Key::new(key.to_string()))
            .map(|v| v.to_string())
    }

    #[test]
    #[serial]
    fn resource_has_service_version_from_cargo_pkg_version_when_env_absent() {
        clear_trace_env();
        let resource = capture_resource();
        assert_eq!(
            attr(&resource, "service.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
        );
    }

    #[test]
    #[serial]
    fn resource_always_has_senko_version_from_cargo_pkg_version() {
        clear_trace_env();
        let resource = capture_resource();
        assert_eq!(
            attr(&resource, "senko.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
        );

        // Even when the operator tries to override it via env, senko.version
        // must stay pinned to the baked-in version.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "senko.version=evil");
        }
        let resource = capture_resource();
        assert_eq!(
            attr(&resource, "senko.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
        );
        clear_trace_env();
    }

    #[test]
    #[serial]
    fn resource_service_version_respects_env_override() {
        clear_trace_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "service.version=1.2.3-foo");
        }
        let resource = capture_resource();
        assert_eq!(
            attr(&resource, "service.version").as_deref(),
            Some("1.2.3-foo"),
        );
        // senko.version stays at the baked-in version.
        assert_eq!(
            attr(&resource, "senko.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
        );
        clear_trace_env();
    }

    // --- resolve_otlp_protocol ---

    /// Clear every OTLP protocol env var the resolver looks at. Callers MUST
    /// be `#[serial]`.
    fn clear_otlp_protocol_env() {
        // SAFETY: callers are marked #[serial].
        unsafe {
            std::env::remove_var("OTEL_EXPORTER_OTLP_PROTOCOL");
            std::env::remove_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL");
            std::env::remove_var("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL");
        }
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_default_when_unset() {
        clear_otlp_protocol_env();
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_grpc_value() {
        clear_otlp_protocol_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );
        clear_otlp_protocol_env();
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_http_protobuf_value() {
        clear_otlp_protocol_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::HttpProtobuf),
        );
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"),
            Ok(OtlpExportProtocol::HttpProtobuf),
        );
        clear_otlp_protocol_env();
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_signal_overrides_global() {
        clear_otlp_protocol_env();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
            std::env::set_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/protobuf");
        }
        // Traces signal var wins over the global one.
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::HttpProtobuf),
        );
        // Logs signal isn't set, so it falls back to the global `grpc`.
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );
        clear_otlp_protocol_env();
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_signal_alone() {
        clear_otlp_protocol_env();
        // Global unset, only the per-signal var is set.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "http/protobuf");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"),
            Ok(OtlpExportProtocol::HttpProtobuf),
        );
        // Traces signal still falls back to the default Grpc.
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );
        clear_otlp_protocol_env();
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_invalid_value() {
        clear_otlp_protocol_env();
        // `http/json` is intentionally out of scope and treated as invalid.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Err("http/json".to_string()),
        );

        // Arbitrary garbage is also an error.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "tcp");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"),
            Err("tcp".to_string()),
        );
        clear_otlp_protocol_env();
    }

    #[test]
    #[serial]
    fn resolve_otlp_protocol_empty_string_treated_as_unset() {
        clear_otlp_protocol_env();
        // Mirrors `resolve_exporter_choice`'s empty-string handling.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "");
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::Grpc),
        );

        // Empty signal var falls through to the global, not into Err.
        // SAFETY: test is #[serial].
        unsafe {
            std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf");
            std::env::set_var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "");
        }
        assert_eq!(
            resolve_otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL"),
            Ok(OtlpExportProtocol::HttpProtobuf),
        );
        clear_otlp_protocol_env();
    }
}
