use std::collections::BTreeMap;

use super::trace_propagation::{build_baggage_header, new_traceparent};
use super::{INBOUND_BAGGAGE, PASSTHROUGH_TOKEN};
use crate::domain::project::ProjectId;

/// Shared HTTP client encapsulating base URL, reqwest client, optional API key,
/// and the static trace attributes to re-emit on every outbound request.
///
/// Used by `Remote*Operations` via composition.
pub(crate) struct HttpClient {
    base_url: String,
    client: reqwest::Client,
    api_key: Option<String>,
    /// Static trace attributes configured at construction time (CLI `--attr`,
    /// `OTEL_RESOURCE_ATTRIBUTES`, etc.). On every `propagate()` call these
    /// are merged with the per-request `INBOUND_BAGGAGE` task-local (see
    /// `super::INBOUND_BAGGAGE`) so relay mode forwards CLI-origin baggage
    /// to the upstream; inbound entries win on key conflict.
    attributes: BTreeMap<String, String>,
}

impl HttpClient {
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        attributes: BTreeMap<String, String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            api_key,
            attributes,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub fn project_url(&self, project_id: ProjectId, path: &str) -> String {
        format!("{}/api/v1/projects/{project_id}{path}", self.base_url)
    }

    /// Attach Bearer authentication to a request builder.
    ///
    /// Priority: explicit api_key > PASSTHROUGH_TOKEN task-local > no auth.
    pub fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(key) = &self.api_key {
            return builder.bearer_auth(key);
        }
        if let Ok(token) = PASSTHROUGH_TOKEN.try_with(|t| t.clone()) {
            return builder.bearer_auth(token);
        }
        builder
    }

    /// Attach W3C Trace Context + Baggage headers to a request builder.
    ///
    /// - `traceparent` is always added (freshly generated per call so each HTTP
    ///   request is its own span on the server side).
    /// - `baggage` is built per-call by merging the static `attributes` with
    ///   any `INBOUND_BAGGAGE` task-local (set by `propagate_trace_context`
    ///   middleware). Inbound entries win on key conflict so relay mode
    ///   forwards CLI-origin baggage. Omitted when the merged map is empty.
    pub fn propagate(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.header("traceparent", new_traceparent().header);
        let merged = self.merged_attributes();
        if let Some(baggage) = build_baggage_header(&merged) {
            builder = builder.header("baggage", baggage);
        }
        builder
    }

    /// Static attrs merged with the per-request `INBOUND_BAGGAGE` task-local.
    /// Inbound wins on key conflict; absence of the task-local (no active
    /// scope) yields just the static attrs.
    fn merged_attributes(&self) -> BTreeMap<String, String> {
        let mut merged = self.attributes.clone();
        let _ = INBOUND_BAGGAGE.try_with(|inbound| {
            for (k, v) in inbound.iter() {
                merged.insert(k.clone(), v.clone());
            }
        });
        merged
    }

    pub fn reqwest(&self) -> &reqwest::Client {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_with(attrs: BTreeMap<String, String>) -> HttpClient {
        HttpClient::new("http://localhost:0", None, attrs)
    }

    fn build_headers(client: &HttpClient) -> reqwest::header::HeaderMap {
        let req = client
            .propagate(client.reqwest().get("http://localhost:0/"))
            .build()
            .expect("build request");
        req.headers().clone()
    }

    #[test]
    fn propagate_always_adds_traceparent() {
        let client = client_with(BTreeMap::new());
        let headers = build_headers(&client);
        let tp = headers.get("traceparent").expect("traceparent missing");
        let tp = tp.to_str().unwrap();
        // Format: 00-<32hex>-<16hex>-01
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(parts.len(), 4, "traceparent shape: {tp}");
        assert_eq!(parts[0], "00");
        assert_eq!(parts[1].len(), 32);
        assert_eq!(parts[2].len(), 16);
        assert_eq!(parts[3], "01");
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn propagate_omits_baggage_when_attributes_empty() {
        let client = client_with(BTreeMap::new());
        let headers = build_headers(&client);
        assert!(headers.get("baggage").is_none());
    }

    #[test]
    fn propagate_adds_baggage_when_attributes_present() {
        let mut attrs = BTreeMap::new();
        attrs.insert("run.id".to_string(), "abc".to_string());
        attrs.insert("session.id".to_string(), "xyz".to_string());
        let client = client_with(attrs);
        let headers = build_headers(&client);
        let baggage = headers
            .get("baggage")
            .expect("baggage missing")
            .to_str()
            .unwrap()
            .to_string();
        // BTreeMap ordering: run.id then session.id.
        // NON_ALPHANUMERIC percent-encodes `.` as %2E.
        assert_eq!(baggage, "run%2Eid=abc,session%2Eid=xyz");
    }

    #[test]
    fn propagate_generates_fresh_traceparent_each_call() {
        let client = client_with(BTreeMap::new());
        let h1 = build_headers(&client);
        let h2 = build_headers(&client);
        let t1 = h1.get("traceparent").unwrap().to_str().unwrap().to_string();
        let t2 = h2.get("traceparent").unwrap().to_str().unwrap().to_string();
        assert_ne!(t1, t2, "expected distinct traceparent values");
    }

    #[test]
    fn auth_and_propagate_are_independent() {
        let mut attrs = BTreeMap::new();
        attrs.insert("k".to_string(), "v".to_string());
        let client = HttpClient::new("http://localhost:0", Some("tok".into()), attrs);
        let req = client
            .propagate(client.auth(client.reqwest().get("http://localhost:0/")))
            .build()
            .expect("build");
        let headers = req.headers();
        assert_eq!(headers.get("authorization").unwrap(), "Bearer tok");
        assert!(headers.get("traceparent").is_some());
        assert_eq!(headers.get("baggage").unwrap(), "k=v");
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn propagate_reads_inbound_baggage_from_task_local() {
        // Relay scenario: static attrs empty, inbound baggage carries the
        // CLI-origin keys. The outbound request should re-emit them.
        let client = client_with(BTreeMap::new());
        let mut inbound = BTreeMap::new();
        inbound.insert("run.id".to_string(), "abc".to_string());
        inbound.insert("session.id".to_string(), "xyz".to_string());

        let baggage = rt().block_on(INBOUND_BAGGAGE.scope(inbound, async {
            let headers = build_headers(&client);
            headers
                .get("baggage")
                .expect("baggage missing")
                .to_str()
                .unwrap()
                .to_string()
        }));

        assert_eq!(baggage, "run%2Eid=abc,session%2Eid=xyz");
    }

    #[test]
    fn propagate_merges_static_and_inbound_with_inbound_winning() {
        // Static has `run.id=static`, inbound has `run.id=inbound` + an extra
        // key. Merge should contain `run.id=inbound` (inbound wins) plus the
        // extra key.
        let mut static_attrs = BTreeMap::new();
        static_attrs.insert("run.id".to_string(), "static".to_string());
        static_attrs.insert("host".to_string(), "relay".to_string());
        let client = client_with(static_attrs);

        let mut inbound = BTreeMap::new();
        inbound.insert("run.id".to_string(), "inbound".to_string());
        inbound.insert("extra".to_string(), "new".to_string());

        let baggage = rt().block_on(INBOUND_BAGGAGE.scope(inbound, async {
            let headers = build_headers(&client);
            headers
                .get("baggage")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        }));

        // BTreeMap order: extra, host, run.id
        assert_eq!(baggage, "extra=new,host=relay,run%2Eid=inbound");
    }

    #[test]
    fn propagate_without_task_local_scope_uses_only_static_attrs() {
        // Outside any INBOUND_BAGGAGE scope, HttpClient falls back to static
        // attrs only — identical to pre-relay behavior.
        let mut attrs = BTreeMap::new();
        attrs.insert("k".to_string(), "v".to_string());
        let client = client_with(attrs);
        let headers = build_headers(&client);
        assert_eq!(headers.get("baggage").unwrap(), "k=v");
    }

    #[test]
    fn propagate_with_empty_scope_and_empty_static_omits_baggage() {
        let client = client_with(BTreeMap::new());
        let result = rt().block_on(INBOUND_BAGGAGE.scope(BTreeMap::new(), async {
            let headers = build_headers(&client);
            headers.get("baggage").is_some()
        }));
        assert!(
            !result,
            "expected no baggage header when both sources empty"
        );
    }
}
