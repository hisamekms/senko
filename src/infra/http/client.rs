use std::collections::BTreeMap;

use super::PASSTHROUGH_TOKEN;
use super::trace_propagation::{build_baggage_header, new_traceparent};
use crate::domain::project::ProjectId;

/// Shared HTTP client encapsulating base URL, reqwest client, optional API key,
/// and a pre-built Baggage header derived from the merged trace attributes.
///
/// Used by `Remote*Operations` via composition.
pub(crate) struct HttpClient {
    base_url: String,
    client: reqwest::Client,
    api_key: Option<String>,
    /// Pre-built `baggage` header value. `None` when the merged attribute map
    /// is empty (caller omits the header entirely).
    baggage_header: Option<String>,
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
            baggage_header: build_baggage_header(&attributes),
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
    /// - `baggage` is added only when the merged attribute map was non-empty at
    ///   construction.
    pub fn propagate(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.header("traceparent", new_traceparent().header);
        if let Some(ref baggage) = self.baggage_header {
            builder = builder.header("baggage", baggage);
        }
        builder
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
}
