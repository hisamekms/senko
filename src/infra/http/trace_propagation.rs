//! Pure functions for W3C Baggage + Trace Context propagation shared by
//! CLI and Remote. No I/O, no globals — everything takes inputs and returns
//! strings / maps.

use std::collections::BTreeMap;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use uuid::Uuid;

/// OTel semantic-convention namespaces that identify the *server* resource.
/// We strip these from `OTEL_RESOURCE_ATTRIBUTES` before propagating to the
/// Remote — otherwise the client's `service.name=senko-cli` would overwrite
/// the Remote's own resource. `SENKO_TRACE_ATTRIBUTES` and `--attr` bypass
/// this filter (explicit user intent).
const RESERVED_NAMESPACES: &[&str] = &[
    "service.",
    "host.",
    "os.",
    "process.",
    "telemetry.",
    "deployment.",
    "cloud.",
    "k8s.",
    "container.",
];

pub fn is_reserved_namespace(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    RESERVED_NAMESPACES.iter().any(|ns| lower.starts_with(ns))
}

/// Parse the OTel `OTEL_RESOURCE_ATTRIBUTES` env-var format:
/// comma-separated `key=value` pairs. Malformed entries (missing `=`,
/// empty key, empty value) are silently skipped per the OTel spec's
/// "MUST ignore invalid entries". Insertion order is preserved so later
/// duplicates overwrite earlier ones deterministically downstream.
pub fn parse_otel_resource_attributes(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((k, v)) = entry.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() || v.is_empty() {
            continue;
        }
        out.push((key.to_string(), v.to_string()));
    }
    out
}

/// Merge attribute sources with precedence
/// `cli_attrs > senko_env > otel_env(filtered) > auto`. Reserved-namespace
/// filtering applies **only** to `otel_env` — explicit user overrides
/// via `SENKO_TRACE_ATTRIBUTES` / `--attr` and internally auto-populated
/// entries (e.g. `senko.operation.id`) are respected verbatim.
pub fn merge_attributes(
    cli_attrs: Vec<(String, String)>,
    senko_env: Vec<(String, String)>,
    otel_env: Vec<(String, String)>,
    auto: Vec<(String, String)>,
) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    for (k, v) in auto {
        merged.insert(k, v);
    }
    for (k, v) in otel_env {
        if is_reserved_namespace(&k) {
            continue;
        }
        merged.insert(k, v);
    }
    for (k, v) in senko_env {
        merged.insert(k, v);
    }
    for (k, v) in cli_attrs {
        merged.insert(k, v);
    }
    merged
}

/// Build a W3C Baggage header value. Returns `None` when `attrs` is empty so
/// callers omit the header entirely. Keys and values are percent-encoded with
/// `NON_ALPHANUMERIC` (matching the rest of `src/infra/http/`) — conservative
/// but always safe for the `=`, `,`, and whitespace separators Baggage uses.
pub fn build_baggage_header(attrs: &BTreeMap<String, String>) -> Option<String> {
    if attrs.is_empty() {
        return None;
    }
    let encoded: Vec<String> = attrs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                utf8_percent_encode(k, NON_ALPHANUMERIC),
                utf8_percent_encode(v, NON_ALPHANUMERIC),
            )
        })
        .collect();
    Some(encoded.join(","))
}

/// W3C Trace Context `traceparent` value plus the raw id hex strings,
/// so callers can use the same ids for local logging / OTel span emission
/// without reparsing.
pub struct Traceparent {
    pub header: String,
    pub trace_id: String,
    pub span_id: String,
}

pub fn new_traceparent() -> Traceparent {
    let trace_bytes = *Uuid::new_v4().as_bytes();
    let span_bytes: [u8; 8] = Uuid::new_v4().as_bytes()[..8].try_into().unwrap();
    let trace_id = hex_lower(&trace_bytes);
    let span_id = hex_lower(&span_bytes);
    let header = format!("00-{trace_id}-{span_id}-01");
    Traceparent {
        header,
        trace_id,
        span_id,
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_namespaces_match_every_prefix() {
        for prefix in [
            "service.name",
            "host.arch",
            "os.type",
            "process.pid",
            "telemetry.sdk.name",
            "deployment.environment",
            "cloud.provider",
            "k8s.pod.name",
            "container.id",
        ] {
            assert!(
                is_reserved_namespace(prefix),
                "expected {prefix} to be reserved"
            );
        }
    }

    #[test]
    fn reserved_namespaces_reject_non_matching_keys() {
        assert!(!is_reserved_namespace("run.id"));
        assert!(!is_reserved_namespace("session.id"));
        assert!(!is_reserved_namespace("servicefoo")); // no dot
        assert!(!is_reserved_namespace("my.service.name")); // prefix doesn't start at position 0
        assert!(!is_reserved_namespace(""));
    }

    #[test]
    fn reserved_namespaces_match_uppercase_variants() {
        // Each reserved prefix should match regardless of ASCII case so that an
        // attacker cannot bypass the filter via `Service.name` or `SERVICE.NAME`.
        for (mixed, upper, lower) in [
            ("Service.name", "SERVICE.NAME", "service.name"),
            ("Host.arch", "HOST.ARCH", "host.arch"),
            ("Os.type", "OS.TYPE", "os.type"),
            ("Process.pid", "PROCESS.PID", "process.pid"),
            (
                "Telemetry.sdk.name",
                "TELEMETRY.SDK.NAME",
                "telemetry.sdk.name",
            ),
            (
                "Deployment.environment",
                "DEPLOYMENT.ENVIRONMENT",
                "deployment.environment",
            ),
            ("Cloud.provider", "CLOUD.PROVIDER", "cloud.provider"),
            ("K8s.pod.name", "K8S.POD.NAME", "k8s.pod.name"),
            ("Container.id", "CONTAINER.ID", "container.id"),
        ] {
            assert!(is_reserved_namespace(mixed), "expected {mixed} reserved");
            assert!(is_reserved_namespace(upper), "expected {upper} reserved");
            assert!(is_reserved_namespace(lower), "expected {lower} reserved");
        }
    }

    #[test]
    fn reserved_namespaces_reject_uppercase_when_prefix_not_at_start() {
        assert!(!is_reserved_namespace("MyService.name"));
        assert!(!is_reserved_namespace("MY.SERVICE.NAME"));
        assert!(!is_reserved_namespace("Servicefoo")); // no dot, mixed case
    }

    #[test]
    fn merge_filters_reserved_namespace_from_otel_case_insensitive() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[]),
            pairs(&[("Service.name", "svc"), ("run.id", "X")]),
            pairs(&[]),
        );
        assert_eq!(merged.get("Service.name"), None);
        assert_eq!(merged.get("run.id"), Some(&"X".to_string()));
    }

    #[test]
    fn parse_otel_happy_path() {
        assert_eq!(
            parse_otel_resource_attributes("a=1,b=2"),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ],
        );
    }

    #[test]
    fn parse_otel_whitespace_around_entries() {
        assert_eq!(
            parse_otel_resource_attributes(" k =v , x = y "),
            vec![
                ("k".to_string(), "v".to_string()),
                ("x".to_string(), " y".to_string())
            ],
        );
    }

    #[test]
    fn parse_otel_equals_in_value() {
        assert_eq!(
            parse_otel_resource_attributes("k=a=b"),
            vec![("k".to_string(), "a=b".to_string())],
        );
    }

    #[test]
    fn parse_otel_skips_malformed() {
        assert_eq!(
            parse_otel_resource_attributes("nope,=val,k=,ok=1"),
            vec![("ok".to_string(), "1".to_string())],
        );
    }

    #[test]
    fn parse_otel_empty_input() {
        assert_eq!(
            parse_otel_resource_attributes(""),
            Vec::<(String, String)>::new()
        );
    }

    fn pairs(kvs: &[(&str, &str)]) -> Vec<(String, String)> {
        kvs.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn merge_cli_overrides_senko() {
        let merged = merge_attributes(
            pairs(&[("run.id", "A")]),
            pairs(&[("run.id", "B")]),
            pairs(&[]),
            pairs(&[]),
        );
        assert_eq!(merged.get("run.id"), Some(&"A".to_string()));
    }

    #[test]
    fn merge_senko_overrides_otel() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[("run.id", "B")]),
            pairs(&[("run.id", "C")]),
            pairs(&[]),
        );
        assert_eq!(merged.get("run.id"), Some(&"B".to_string()));
    }

    #[test]
    fn merge_filters_reserved_namespace_from_otel() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[]),
            pairs(&[("service.name", "svc"), ("run.id", "X")]),
            pairs(&[]),
        );
        assert_eq!(merged.get("service.name"), None);
        assert_eq!(merged.get("run.id"), Some(&"X".to_string()));
    }

    #[test]
    fn merge_does_not_filter_reserved_from_senko() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[("service.name", "svc")]),
            pairs(&[]),
            pairs(&[]),
        );
        assert_eq!(merged.get("service.name"), Some(&"svc".to_string()));
    }

    #[test]
    fn merge_does_not_filter_reserved_from_cli() {
        let merged = merge_attributes(
            pairs(&[("service.name", "svc")]),
            pairs(&[]),
            pairs(&[]),
            pairs(&[]),
        );
        assert_eq!(merged.get("service.name"), Some(&"svc".to_string()));
    }

    #[test]
    fn merge_auto_is_lowest_priority() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[]),
            pairs(&[("k", "from-otel")]),
            pairs(&[("k", "from-auto")]),
        );
        assert_eq!(merged.get("k"), Some(&"from-otel".to_string()));
    }

    #[test]
    fn merge_auto_survives_when_no_other_source_sets_it() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[]),
            pairs(&[]),
            pairs(&[("senko.operation.id", "u")]),
        );
        assert_eq!(merged.get("senko.operation.id"), Some(&"u".to_string()),);
    }

    #[test]
    fn merge_auto_not_filtered_by_reserved_namespace() {
        let merged = merge_attributes(
            pairs(&[]),
            pairs(&[]),
            pairs(&[]),
            pairs(&[("service.name", "x")]),
        );
        assert_eq!(merged.get("service.name"), Some(&"x".to_string()));
    }

    #[test]
    fn baggage_empty_returns_none() {
        assert_eq!(build_baggage_header(&BTreeMap::new()), None);
    }

    #[test]
    fn baggage_simple_pair() {
        let mut m = BTreeMap::new();
        m.insert("k".to_string(), "v".to_string());
        assert_eq!(build_baggage_header(&m).as_deref(), Some("k=v"));
    }

    #[test]
    fn baggage_percent_encodes_specials() {
        let mut m = BTreeMap::new();
        m.insert("run.id".to_string(), "a b,c=d".to_string());
        let out = build_baggage_header(&m).unwrap();
        // NON_ALPHANUMERIC encodes `.` as %2E, ` ` as %20, `,` as %2C, `=` as %3D.
        assert_eq!(out, "run%2Eid=a%20b%2Cc%3Dd");
    }

    #[test]
    fn baggage_sorted_order() {
        let mut m = BTreeMap::new();
        m.insert("b".to_string(), "2".to_string());
        m.insert("a".to_string(), "1".to_string());
        assert_eq!(build_baggage_header(&m).as_deref(), Some("a=1,b=2"));
    }

    #[test]
    fn traceparent_format_and_lengths() {
        let tp = new_traceparent();
        assert_eq!(tp.trace_id.len(), 32, "trace_id={}", tp.trace_id);
        assert_eq!(tp.span_id.len(), 16, "span_id={}", tp.span_id);
        assert_eq!(tp.header, format!("00-{}-{}-01", tp.trace_id, tp.span_id));
        assert!(
            tp.trace_id
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(
            tp.span_id
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn traceparent_ids_differ_between_calls() {
        let a = new_traceparent();
        let b = new_traceparent();
        assert_ne!(a.trace_id, b.trace_id);
        assert_ne!(a.span_id, b.span_id);
    }
}
