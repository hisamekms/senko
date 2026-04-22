use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value, json};

use crate::application::HookTrigger;
use crate::application::port::{ContractOperations, HookExecutor};
use crate::domain::contract::{
    Contract, ContractEvent, ContractNote, CreateContractParams, UpdateContractArrayParams,
    UpdateContractParams,
};
use crate::domain::error::DomainError;
use crate::domain::task::MetadataUpdate;
use crate::infra::config::HookWhen;
use crate::infra::hook::FireOutcome;

use super::client::HttpClient;
use super::{check_success, read_json_or_error};

/// HTTP client implementing `ContractOperations`.
pub struct RemoteContractOperations {
    http: HttpClient,
    hooks: Arc<dyn HookExecutor>,
}

impl RemoteContractOperations {
    pub fn new(base_url: &str, api_key: Option<String>, hooks: Arc<dyn HookExecutor>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
            hooks,
        }
    }

    fn project_url(&self, project_id: i64, path: &str) -> String {
        self.http.project_url(project_id, path)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.auth(builder)
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }

    async fn fire_pre(&self, trigger: &HookTrigger, contract: Option<&Contract>) -> Result<()> {
        if self
            .hooks
            .fire_contract(trigger, HookWhen::Pre, contract)
            .await
            == FireOutcome::Abort
        {
            let name = trigger.event_name().unwrap_or("contract");
            return Err(DomainError::HookAborted { event: name.into() }.into());
        }
        Ok(())
    }

    async fn fire_post(&self, trigger: &HookTrigger, contract: Option<&Contract>) {
        let _ = self
            .hooks
            .fire_contract(trigger, HookWhen::Post, contract)
            .await;
    }
}

fn create_params_to_json(p: &CreateContractParams) -> Value {
    json!({
        "title": p.title,
        "description": p.description,
        "definition_of_done": p.definition_of_done,
        "tags": p.tags,
        "metadata": p.metadata,
    })
}

fn update_body(params: &UpdateContractParams, array_params: &UpdateContractArrayParams) -> Value {
    let mut map = Map::new();

    if let Some(ref title) = params.title {
        map.insert("title".into(), json!(title));
    }
    if let Some(ref desc) = params.description {
        match desc {
            None => {
                map.insert("clear_description".into(), json!(true));
            }
            Some(v) => {
                map.insert("description".into(), json!(v));
            }
        }
    }
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

    if let Some(ref tags) = array_params.set_tags {
        map.insert("set_tags".into(), json!(tags));
    }
    if !array_params.add_tags.is_empty() {
        map.insert("add_tags".into(), json!(array_params.add_tags));
    }
    if !array_params.remove_tags.is_empty() {
        map.insert("remove_tags".into(), json!(array_params.remove_tags));
    }
    if let Some(ref dod) = array_params.set_definition_of_done {
        map.insert("set_definition_of_done".into(), json!(dod));
    }
    if !array_params.add_definition_of_done.is_empty() {
        map.insert(
            "add_definition_of_done".into(),
            json!(array_params.add_definition_of_done),
        );
    }
    if !array_params.remove_definition_of_done.is_empty() {
        map.insert(
            "remove_definition_of_done".into(),
            json!(array_params.remove_definition_of_done),
        );
    }

    Value::Object(map)
}

#[async_trait]
impl ContractOperations for RemoteContractOperations {
    async fn create_contract(
        &self,
        project_id: i64,
        params: &CreateContractParams,
    ) -> Result<Contract> {
        let trigger = HookTrigger::Contract(ContractEvent::Created);
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(project_id, "/contracts");
        let resp = self
            .auth(
                self.client()
                    .post(&url)
                    .json(&create_params_to_json(params)),
            )
            .send()
            .await?;
        let contract: Contract = read_json_or_error(resp).await?;

        self.fire_post(&trigger, Some(&contract)).await;
        Ok(contract)
    }

    async fn get_contract(&self, project_id: i64, id: i64) -> Result<Contract> {
        let url = self.project_url(project_id, &format!("/contracts/{id}"));
        let resp = self.auth(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn list_contracts(&self, project_id: i64) -> Result<Vec<Contract>> {
        let url = self.project_url(project_id, "/contracts");
        let resp = self.auth(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn edit_contract(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateContractParams,
        array_params: &UpdateContractArrayParams,
    ) -> Result<Contract> {
        let trigger = HookTrigger::Contract(ContractEvent::Updated);
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(project_id, &format!("/contracts/{id}"));
        let body = update_body(params, array_params);
        let resp = self
            .auth(self.client().put(&url).json(&body))
            .send()
            .await?;
        let contract: Contract = read_json_or_error(resp).await?;

        self.fire_post(&trigger, Some(&contract)).await;
        Ok(contract)
    }

    async fn delete_contract(&self, project_id: i64, id: i64) -> Result<()> {
        let trigger = HookTrigger::Contract(ContractEvent::Deleted);
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(project_id, &format!("/contracts/{id}"));
        let resp = self.auth(self.client().delete(&url)).send().await?;
        check_success(resp).await?;

        self.fire_post(&trigger, None).await;
        Ok(())
    }

    async fn check_dod(&self, project_id: i64, contract_id: i64, index: usize) -> Result<Contract> {
        let trigger = HookTrigger::Contract(ContractEvent::DodChecked { index });
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(
            project_id,
            &format!("/contracts/{contract_id}/dod/{index}/check"),
        );
        let resp = self.auth(self.client().post(&url)).send().await?;
        let contract: Contract = read_json_or_error(resp).await?;

        self.fire_post(&trigger, Some(&contract)).await;
        Ok(contract)
    }

    async fn uncheck_dod(
        &self,
        project_id: i64,
        contract_id: i64,
        index: usize,
    ) -> Result<Contract> {
        let trigger = HookTrigger::Contract(ContractEvent::DodUnchecked { index });
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(
            project_id,
            &format!("/contracts/{contract_id}/dod/{index}/uncheck"),
        );
        let resp = self.auth(self.client().post(&url)).send().await?;
        let contract: Contract = read_json_or_error(resp).await?;

        self.fire_post(&trigger, Some(&contract)).await;
        Ok(contract)
    }

    async fn add_note(
        &self,
        project_id: i64,
        contract_id: i64,
        content: String,
        source_task_id: Option<crate::domain::task::TaskId>,
    ) -> Result<ContractNote> {
        let trigger = HookTrigger::Contract(ContractEvent::NoteAdded);
        self.fire_pre(&trigger, None).await?;

        let url = self.project_url(project_id, &format!("/contracts/{contract_id}/notes"));
        let body = json!({
            "content": content,
            "source_task_id": source_task_id,
        });
        let resp = self
            .auth(self.client().post(&url).json(&body))
            .send()
            .await?;
        let note: ContractNote = read_json_or_error(resp).await?;

        self.fire_post(&trigger, None).await;
        Ok(note)
    }

    async fn list_notes(&self, project_id: i64, contract_id: i64) -> Result<Vec<ContractNote>> {
        let url = self.project_url(project_id, &format!("/contracts/{contract_id}/notes"));
        let resp = self.auth(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }
}
