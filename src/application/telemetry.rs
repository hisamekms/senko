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
//! Common attributes (`senko.operation.id`, `enduser.*`, Resource attrs)
//! are NOT taken here — Phase B2 will auto-attach them via baggage,
//! task-locals, or span attributes. Only callsite-specific attributes go
//! into the macro.

/// Emit one Contract #8 business event as an OTel `LogRecord`.
///
/// The first argument is the OTel `event.name` (e.g. `"senko.task.published"`).
/// Remaining arguments are forwarded verbatim to [`tracing::event!`] as
/// structured fields, including dotted field names such as
/// `senko.task.id = 42`.
///
/// # Example
///
/// ```ignore
/// emit_business_event!(
///     "senko.task.published",
///     senko.task.id = 42_i64,
///     from_status = "todo",
///     to_status = "in_progress"
/// );
/// ```
#[macro_export]
macro_rules! emit_business_event {
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
}
