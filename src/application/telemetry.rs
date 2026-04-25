//! Business-event emission for Contract #8 OTel observability.
//!
//! [`emit_business_event!`](crate::emit_business_event) is the single emit
//! hook for the 29 `senko.*` business events defined in Contract #8
//! (`senko.task.published`, `senko.project.member_added`,
//! `senko.user.api_key_issued`, …).
//!
//! Internally it expands to a [`tracing::event!`] call with:
//! - `name:` — set to the OTel event name; the
//!   [`opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge`]
//!   layer reads it from `Metadata::name()` and forwards it via
//!   `LogRecord::set_event_name`.
//! - `target: "senko_business"` — lets `RUST_LOG` filter business events
//!   independently of infrastructure tracing (`info!` / `warn!` callers
//!   keep the default module target).
//! - `Level::INFO` — fixed; errors are emitted separately as
//!   `senko.api.error` (Phase C2).
//!
//! Common attributes (`senko.operation.id`, `enduser.*`, caller-supplied
//! baggage like `aviary.session.id`) are auto-attached by
//! [`BusinessAttributesProcessor`] — a [`LogProcessor`] that reads two
//! task-locals on `emit`:
//! - [`RESOLVED_USER`] — set by Phase C3's auth middleware after the
//!   inbound principal is resolved; supplies `enduser.id` / `enduser.name`.
//! - `crate::infra::http::INBOUND_BAGGAGE` — set by `propagate_trace_context`
//!   middleware from the W3C `baggage` header; supplies `senko.operation.id`
//!   and any caller-supplied attributes verbatim (no `baggage.` prefix).
//!
//! The processor is wired into [`crate::bootstrap::init_telemetry`] **before**
//! the export processor; tests plug it in the same way. Resource attributes
//! (`service.name` / `service.version` / `senko.version`) are attached by
//! the SDK at `Resource` level, not here.
//!
//! Callsites pass only the event-specific attributes (e.g. `senko.task.id`,
//! `from_status`); the processor enriches every `senko_business` record with
//! the per-request common attributes above.

/// Emit one Contract #8 business event as an OTel `LogRecord`.
///
/// The first argument is the OTel `event.name` (e.g. `"senko.task.published"`).
/// Remaining arguments are forwarded verbatim to [`tracing::event!`] as
/// structured fields, including dotted field names such as
/// `senko.task.id = 42`.
///
/// Defaults to `Level::INFO`. For error-level events (currently only
/// `senko.api.error`), prefix the field list with `level: ERROR`.
///
/// # Examples
///
/// ```ignore
/// emit_business_event!(
///     "senko.task.published",
///     senko.task.id = 42_i64,
///     from_status = "todo",
///     to_status = "in_progress"
/// );
///
/// emit_business_event!(
///     "senko.api.error",
///     level: ERROR,
///     http.status_code = 500_u16,
///     error.type = "internal",
///     error.message = %err,
/// );
/// ```
#[macro_export]
macro_rules! emit_business_event {
    // ERROR level, no fields (with optional trailing comma).
    // Must precede the INFO arms — macro_rules! tries declarations top-down
    // and the INFO `$($fields:tt)*` arm would otherwise greedily swallow
    // `level: ERROR` as a field list.
    ($otel_event_name:expr, level: ERROR $(,)?) => {
        ::tracing::event!(
            name: $otel_event_name,
            target: "senko_business",
            ::tracing::Level::ERROR,
            {}
        );
    };
    ($otel_event_name:expr, level: ERROR, $($fields:tt)*) => {
        ::tracing::event!(
            name: $otel_event_name,
            target: "senko_business",
            ::tracing::Level::ERROR,
            $($fields)*
        );
    };
    ($otel_event_name:expr) => {
        ::tracing::event!(
            name: $otel_event_name,
            target: "senko_business",
            ::tracing::Level::INFO,
            {}
        );
    };
    ($otel_event_name:expr, $($fields:tt)*) => {
        ::tracing::event!(
            name: $otel_event_name,
            target: "senko_business",
            ::tracing::Level::INFO,
            $($fields)*
        );
    };
}

use std::time::Duration;

use opentelemetry::InstrumentationScope;
use opentelemetry::logs::LogRecord as _;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogProcessor, SdkLogRecord};

/// Auth-resolved enduser identity for the in-flight request.
///
/// Populated by Phase C3's auth middleware via
/// `RESOLVED_USER.scope(user, fut).await` once the inbound principal is
/// known; consumed by [`BusinessAttributesProcessor`] to auto-attach
/// `enduser.id` / `enduser.name` to every `senko_business` LogRecord
/// emitted under that scope.
#[derive(Debug, Clone)]
pub struct ResolvedUser {
    pub id: i64,
    pub username: String,
}

tokio::task_local! {
    /// Per-request resolved user. Outside the scope `try_with` returns
    /// `Err(AccessError)` and the processor silently skips `enduser.*`
    /// attribution — emit paths that bypass auth (CLI, tests) won't crash.
    pub static RESOLVED_USER: ResolvedUser;
}

/// LogProcessor that enriches Contract #8 business-event records with
/// per-request common attributes pulled from task-locals.
///
/// Wired BEFORE the export processor in `SdkLoggerProvider::builder()` so
/// the exporter sees the enriched record; mirrors the
/// `EnrichWithBaggageProcessor` pattern in the OpenTelemetry SDK 0.31
/// docs (`opentelemetry_sdk::logs::tests::log_and_baggage`).
///
/// Records whose `target` is not `"senko_business"` (i.e. infrastructure
/// `tracing::info!` / `warn!` calls) pass through untouched.
#[derive(Debug, Default, Clone, Copy)]
pub struct BusinessAttributesProcessor;

impl LogProcessor for BusinessAttributesProcessor {
    fn emit(&self, record: &mut SdkLogRecord, _scope: &InstrumentationScope) {
        let is_business = matches!(record.target().map(|c| c.as_ref()), Some("senko_business"));
        if !is_business {
            return;
        }

        let _ = RESOLVED_USER.try_with(|u| {
            record.add_attribute("enduser.id", u.id);
            record.add_attribute("enduser.name", u.username.clone());
        });

        let _ = crate::infra::http::INBOUND_BAGGAGE.try_with(|map| {
            for (k, v) in map.iter() {
                record.add_attribute(k.clone(), v.clone());
            }
        });
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: Duration) -> OTelSdkResult {
        Ok(())
    }
}

/// Test-only utilities for verifying business-event emission. Used by every
/// service-level unit test that needs to assert an `emit_business_event!`
/// callsite was reached with the expected `event_name` and attributes.
///
/// Lives outside `mod tests` so other modules' test modules can reach it via
/// `crate::application::telemetry::test_support::*` without re-declaring the
/// SDK plumbing in each file.
#[cfg(test)]
pub(crate) mod test_support {
    use super::BusinessAttributesProcessor;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLogRecord, SdkLoggerProvider};

    /// Build an in-memory logger provider wired with `BusinessAttributesProcessor`
    /// (matches the production order in `bootstrap::build_logger_provider`).
    pub(crate) fn build_capture_provider() -> (InMemoryLogExporter, SdkLoggerProvider) {
        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_log_processor(BusinessAttributesProcessor)
            .with_simple_exporter(exporter.clone())
            .build();
        (exporter, provider)
    }

    /// Build the tracing-subscriber `Layer` that bridges `tracing::event!` calls
    /// to OTel `LogRecord` via `OpenTelemetryTracingBridge`.
    pub(crate) fn capture_layer(
        provider: &SdkLoggerProvider,
    ) -> OpenTelemetryTracingBridge<SdkLoggerProvider, opentelemetry_sdk::logs::SdkLogger> {
        OpenTelemetryTracingBridge::new(provider)
    }

    /// Look up a single attribute on an `SdkLogRecord` by key.
    pub(crate) fn lookup_attr(record: &SdkLogRecord, key: &str) -> Option<AnyValue> {
        record
            .attributes_iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use opentelemetry::logs::{AnyValue, Severity};
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
    use tracing_subscriber::layer::SubscriberExt;

    fn lookup_attr(record: &opentelemetry_sdk::logs::SdkLogRecord, key: &str) -> Option<AnyValue> {
        record
            .attributes_iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn emits_log_record_with_otel_event_name_and_attributes() {
        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        tracing::subscriber::with_default(subscriber, || {
            crate::emit_business_event!(
                "senko.task.published",
                senko.task.id = 42_i64,
                from_status = "todo",
                to_status = "in_progress"
            );
        });

        provider.force_flush().expect("flush ok");

        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1, "expected exactly one log record");
        let record = &logs[0].record;

        assert_eq!(
            record.event_name(),
            Some("senko.task.published"),
            "name: forwarded to LogRecord::event_name via OpenTelemetryTracingBridge"
        );
        assert_eq!(record.target().map(|c| c.as_ref()), Some("senko_business"),);
        assert_eq!(record.severity_number(), Some(Severity::Info));

        assert_eq!(
            lookup_attr(record, "senko.task.id"),
            Some(AnyValue::Int(42))
        );
        assert_eq!(
            lookup_attr(record, "from_status"),
            Some(AnyValue::String("todo".into()))
        );
        assert_eq!(
            lookup_attr(record, "to_status"),
            Some(AnyValue::String("in_progress".into()))
        );
    }

    #[test]
    fn emits_log_record_without_attributes() {
        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        tracing::subscriber::with_default(subscriber, || {
            crate::emit_business_event!("senko.contract.deleted");
        });

        provider.force_flush().expect("flush ok");

        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].record.event_name(), Some("senko.contract.deleted"));
        assert_eq!(
            logs[0].record.target().map(|c| c.as_ref()),
            Some("senko_business")
        );
    }

    // --- BusinessAttributesProcessor enrichment ---------------------------

    use std::collections::BTreeMap;

    use super::{BusinessAttributesProcessor, RESOLVED_USER, ResolvedUser};
    use crate::infra::http::INBOUND_BAGGAGE;

    /// Build a logger provider wired with the enricher BEFORE the simple
    /// exporter — the production order from `bootstrap::build_logger_provider`.
    fn provider_with_enricher() -> (InMemoryLogExporter, SdkLoggerProvider) {
        let exporter = InMemoryLogExporter::default();
        let provider = SdkLoggerProvider::builder()
            .with_log_processor(BusinessAttributesProcessor)
            .with_simple_exporter(exporter.clone())
            .build();
        (exporter, provider)
    }

    #[test]
    fn enricher_attaches_enduser_from_resolved_user_task_local() {
        let (exporter, provider) = provider_with_enricher();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let user = ResolvedUser {
                id: 7,
                username: "alice".into(),
            };
            RESOLVED_USER
                .scope(user, async {
                    let _g = tracing::subscriber::set_default(subscriber);
                    crate::emit_business_event!("senko.task.published", senko.task.id = 1_i64);
                })
                .await;
        });

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1);
        let record = &logs[0].record;

        assert_eq!(lookup_attr(record, "enduser.id"), Some(AnyValue::Int(7)));
        assert_eq!(
            lookup_attr(record, "enduser.name"),
            Some(AnyValue::String("alice".into()))
        );
        // callsite attribute survives alongside auto-attached ones
        assert_eq!(lookup_attr(record, "senko.task.id"), Some(AnyValue::Int(1)));
    }

    #[test]
    fn enricher_attaches_inbound_baggage_with_raw_keys() {
        let (exporter, provider) = provider_with_enricher();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut baggage = BTreeMap::new();
            baggage.insert("senko.operation.id".to_string(), "op-xyz".to_string());
            baggage.insert("aviary.session.id".to_string(), "ses-1".to_string());
            INBOUND_BAGGAGE
                .scope(baggage, async {
                    let _g = tracing::subscriber::set_default(subscriber);
                    crate::emit_business_event!("senko.contract.created");
                })
                .await;
        });

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1);
        let record = &logs[0].record;

        // Raw keys, no `baggage.` prefix — Contract #8 common-attribute names
        assert_eq!(
            lookup_attr(record, "senko.operation.id"),
            Some(AnyValue::String("op-xyz".into()))
        );
        assert_eq!(
            lookup_attr(record, "aviary.session.id"),
            Some(AnyValue::String("ses-1".into()))
        );
        // Defensive: prefix form must NOT appear
        assert!(lookup_attr(record, "baggage.senko.operation.id").is_none());
    }

    #[test]
    fn enricher_no_crash_when_task_locals_unset() {
        let (exporter, provider) = provider_with_enricher();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        // No RESOLVED_USER / INBOUND_BAGGAGE scope around the emit — emulates
        // a CLI / test path where neither auth nor inbound HTTP middleware ran.
        tracing::subscriber::with_default(subscriber, || {
            crate::emit_business_event!(
                "senko.task.canceled",
                senko.task.id = 2_i64,
                from_status = "in_progress"
            );
        });

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1);
        let record = &logs[0].record;

        // Auto-attached attrs absent — neither task-local was scoped
        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
        assert!(lookup_attr(record, "senko.operation.id").is_none());
        // Callsite attrs preserved
        assert_eq!(lookup_attr(record, "senko.task.id"), Some(AnyValue::Int(2)));
        assert_eq!(
            lookup_attr(record, "from_status"),
            Some(AnyValue::String("in_progress".into()))
        );
    }

    #[test]
    fn enricher_skips_records_with_non_business_target() {
        // Wire enricher + exporter, then emit a non-business tracing event
        // (target = module path, not "senko_business") under populated
        // task-locals. The enricher must NOT inject auto-attrs for this record.
        let (exporter, provider) = provider_with_enricher();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let user = ResolvedUser {
                id: 7,
                username: "alice".into(),
            };
            let mut baggage = BTreeMap::new();
            baggage.insert("senko.operation.id".to_string(), "op-xyz".to_string());

            RESOLVED_USER
                .scope(
                    user,
                    INBOUND_BAGGAGE.scope(baggage, async {
                        let _g = tracing::subscriber::set_default(subscriber);
                        // Non-business tracing event: default target = module path.
                        tracing::info!(unrelated_field = 1, "infrastructure log");
                    }),
                )
                .await;
        });

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        assert_eq!(logs.len(), 1);
        let record = &logs[0].record;

        // Sanity: this is not a business record
        assert_ne!(record.target().map(|c| c.as_ref()), Some("senko_business"));
        // Enricher did not inject any auto-attrs
        assert!(lookup_attr(record, "enduser.id").is_none());
        assert!(lookup_attr(record, "enduser.name").is_none());
        assert!(lookup_attr(record, "senko.operation.id").is_none());
    }
}
