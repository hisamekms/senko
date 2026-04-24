pub(crate) mod client;
pub mod remote_contract_ops;
pub mod remote_hook_data;
pub mod remote_metadata_field_ops;
pub mod remote_project_ops;
pub mod remote_task_ops;
pub mod remote_user_ops;
pub mod trace_propagation;

use anyhow::Result;
use serde_json::json;

use crate::domain::task::{MetadataUpdate, UpdateTaskArrayParams, UpdateTaskParams};

tokio::task_local! {
    pub static PASSTHROUGH_TOKEN: String;
    /// Per-request baggage extracted from the inbound `baggage` header by
    /// `propagate_trace_context`. Re-emitted on outbound `HttpClient` requests
    /// so relay mode forwards CLI-origin baggage to the upstream Remote.
    /// Contains the raw key/value pairs without reserved-namespace re-filtering
    /// (the CLI already filters; relay is a passthrough).
    pub static INBOUND_BAGGAGE: std::collections::BTreeMap<String, String>;
}

/// Error type representing a non-success HTTP response from the upstream server.
#[derive(Debug, thiserror::Error)]
#[error("upstream HTTP error {status}: {message}")]
pub struct UpstreamHttpError {
    pub status: reqwest::StatusCode,
    pub message: String,
}

/// Extract error message from a JSON error response body.
pub(crate) async fn extract_error(resp: reqwest::Response) -> String {
    resp.json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| "unknown error".into())
}

/// Read a successful JSON response, or return `UpstreamHttpError` on non-2xx.
pub(crate) async fn read_json_or_error<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T> {
    if resp.status().is_success() {
        Ok(resp.json().await?)
    } else {
        let status = resp.status();
        let message = extract_error(resp).await;
        Err(UpstreamHttpError { status, message }.into())
    }
}

/// Check that a response is successful (2xx), ignoring the body. Return `UpstreamHttpError` on error.
pub(crate) async fn check_success(resp: reqwest::Response) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let message = extract_error(resp).await;
        Err(UpstreamHttpError { status, message }.into())
    }
}

/// Build the JSON body for `PUT /tasks/{id}` from `UpdateTaskParams`.
pub(crate) fn update_params_to_json(params: &UpdateTaskParams) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    if let Some(ref title) = params.title {
        map.insert("title".into(), json!(title));
    }
    if let Some(ref priority) = params.priority {
        map.insert("priority".into(), json!(priority));
    }

    macro_rules! clearable {
        ($field:ident) => {
            if let Some(ref outer) = params.$field {
                match outer {
                    None => {
                        map.insert(concat!("clear_", stringify!($field)).into(), json!(true));
                    }
                    Some(val) => {
                        map.insert(stringify!($field).into(), json!(val));
                    }
                }
            }
        };
    }

    clearable!(background);
    clearable!(description);
    clearable!(plan);
    clearable!(branch);
    clearable!(pr_url);
    // contract_id uses a different key naming: the API expects "contract_id" / "clear_contract"
    if let Some(ref outer) = params.contract_id {
        match outer {
            None => {
                map.insert("clear_contract".into(), json!(true));
            }
            Some(val) => {
                map.insert("contract_id".into(), json!(val));
            }
        }
    }
    // metadata uses MetadataUpdate enum instead of clearable! pattern
    if let Some(ref meta_update) = params.metadata {
        match meta_update {
            MetadataUpdate::Clear => {
                map.insert("clear_metadata".into(), json!(true));
            }
            MetadataUpdate::Merge(v) => {
                map.insert("metadata".into(), json!(v));
            }
            MetadataUpdate::Replace(v) => {
                map.insert("replace_metadata".into(), json!(v));
            }
        }
    }
    clearable!(cancel_reason);
    clearable!(assignee_session_id);
    clearable!(started_at);
    clearable!(completed_at);
    clearable!(canceled_at);

    if let Some(ref outer) = params.assignee_user_id {
        match outer {
            None => {
                map.insert("clear_assignee_user_id".into(), json!(true));
            }
            Some(val) => {
                map.insert("assignee_user_id".into(), json!(val));
            }
        }
    }

    serde_json::Value::Object(map)
}

/// Build the JSON body for `PUT /tasks/{id}` from `UpdateTaskArrayParams`.
pub(crate) fn array_params_to_json(params: &UpdateTaskArrayParams) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    macro_rules! array_field {
        ($set:ident, $add:ident, $remove:ident) => {
            if let Some(ref v) = params.$set {
                map.insert(stringify!($set).into(), json!(v));
            }
            if !params.$add.is_empty() {
                map.insert(stringify!($add).into(), json!(params.$add));
            }
            if !params.$remove.is_empty() {
                map.insert(stringify!($remove).into(), json!(params.$remove));
            }
        };
    }

    array_field!(set_tags, add_tags, remove_tags);
    array_field!(
        set_definition_of_done,
        add_definition_of_done,
        remove_definition_of_done
    );
    array_field!(set_in_scope, add_in_scope, remove_in_scope);
    array_field!(set_out_of_scope, add_out_of_scope, remove_out_of_scope);

    serde_json::Value::Object(map)
}
