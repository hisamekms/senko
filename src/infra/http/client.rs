use super::PASSTHROUGH_TOKEN;
use crate::domain::project::ProjectId;

/// Shared HTTP client encapsulating base URL, reqwest client, and optional API key.
///
/// Used by `Remote*Operations` via composition.
pub(crate) struct HttpClient {
    base_url: String,
    client: reqwest::Client,
    api_key: Option<String>,
}

impl HttpClient {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            api_key,
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

    pub fn reqwest(&self) -> &reqwest::Client {
        &self.client
    }
}
