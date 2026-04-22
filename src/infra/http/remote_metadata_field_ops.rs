use anyhow::Result;
use async_trait::async_trait;

use crate::application::port::MetadataFieldOperations;
use crate::domain::metadata_field::{CreateMetadataFieldParams, MetadataField};
use crate::domain::project::ProjectId;

use super::client::HttpClient;
use super::{check_success, read_json_or_error};

/// HTTP client implementing `MetadataFieldOperations` directly.
///
/// Each method maps to a single API endpoint call. Domain logic is executed
/// server-side; this client only handles HTTP transport.
pub struct RemoteMetadataFieldOperations {
    http: HttpClient,
}

impl RemoteMetadataFieldOperations {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
        }
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.auth(builder)
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }
}

#[async_trait]
impl MetadataFieldOperations for RemoteMetadataFieldOperations {
    async fn create_metadata_field(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let resp = self
            .auth(
                self.client()
                    .post(self.http.project_url(project_id, "/metadata-fields"))
                    .json(params),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_metadata_fields(&self, project_id: ProjectId) -> Result<Vec<MetadataField>> {
        let resp = self
            .auth(
                self.client()
                    .get(self.http.project_url(project_id, "/metadata-fields")),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn delete_metadata_field_by_name(&self, project_id: ProjectId, name: &str) -> Result<()> {
        let resp = self
            .auth(
                self.client().delete(
                    self.http
                        .project_url(project_id, &format!("/metadata-fields/{name}")),
                ),
            )
            .send()
            .await?;
        check_success(resp).await
    }
}
