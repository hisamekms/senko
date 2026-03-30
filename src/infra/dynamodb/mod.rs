use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use chrono::Utc;
use tokio::sync::OnceCell;

use crate::domain::project::{CreateProjectParams, Project};
use crate::application::port::TaskQueryPort;
use crate::domain::error::DomainError;
use crate::domain::{ApiKeyRepository, ProjectRepository, TaskRepository, UserRepository};
use crate::domain::task::{
    CreateTaskParams, DodItem, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, User,
};

pub struct DynamoDbBackend {
    table_name: String,
    region: Option<String>,
    client: OnceCell<Client>,
}

impl DynamoDbBackend {
    pub fn new(table_name: String, region: Option<String>) -> Self {
        Self {
            table_name,
            region,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<&Client> {
        self.client
            .get_or_try_init(|| async {
                let mut config_loader = aws_config::defaults(
                    aws_config::BehaviorVersion::latest(),
                );
                if let Some(ref region) = self.region {
                    config_loader = config_loader.region(
                        aws_config::Region::new(region.clone()),
                    );
                }
                let sdk_config = config_loader.load().await;
                let client = Client::new(&sdk_config);
                self.ensure_table(&client).await?;
                Ok(client)
            })
            .await
    }

    async fn ensure_table(&self, client: &Client) -> Result<()> {
        use aws_sdk_dynamodb::types::{
            AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
        };

        match client.describe_table().table_name(&self.table_name).send().await {
            Ok(_) => return Ok(()),
            Err(err) => {
                let service_err = err.as_service_error();
                if service_err.map_or(true, |e| !e.is_resource_not_found_exception()) {
                    return Err(anyhow::anyhow!("failed to describe table: {err}"));
                }
            }
        }

        client
            .create_table()
            .table_name(&self.table_name)
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("PK")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("SK")
                    .attribute_type(ScalarAttributeType::S)
                    .build()?,
            )
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("PK")
                    .key_type(KeyType::Hash)
                    .build()?,
            )
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("SK")
                    .key_type(KeyType::Range)
                    .build()?,
            )
            .billing_mode(BillingMode::PayPerRequest)
            .send()
            .await
            .context("failed to create DynamoDB table")?;

        // Wait for table to become active
        loop {
            let resp = client
                .describe_table()
                .table_name(&self.table_name)
                .send()
                .await?;
            if let Some(table) = resp.table() {
                if table.table_status() == Some(&aws_sdk_dynamodb::types::TableStatus::Active) {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        Ok(())
    }

    async fn next_id(&self, prefix: &str) -> Result<i64> {
        let client = self.client().await?;
        let resp = client
            .update_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("COUNTER#{prefix}")))
            .key("SK", AttributeValue::S(format!("COUNTER#{prefix}")))
            .update_expression("ADD #v :inc")
            .expression_attribute_names("#v", "value")
            .expression_attribute_values(":inc", AttributeValue::N("1".into()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::UpdatedNew)
            .send()
            .await
            .context("failed to increment counter")?;

        let attrs = resp.attributes().context("no attributes returned from counter update")?;
        let val = attrs
            .get("value")
            .context("counter value missing")?;
        get_n(val).context("invalid counter value")
    }

    async fn put_task(&self, task: &Task) -> Result<()> {
        let client = self.client().await?;
        let item = task_to_item(task);
        client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .context("failed to put task")?;
        Ok(())
    }

    async fn put_item(&self, item: HashMap<String, AttributeValue>) -> Result<()> {
        let client = self.client().await?;
        client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .context("failed to put item")?;
        Ok(())
    }

    async fn scan_all_tasks(&self) -> Result<Vec<Task>> {
        let client = self.client().await?;
        let mut tasks = Vec::new();
        let mut exclusive_start_key = None;

        loop {
            let mut req = client
                .scan()
                .table_name(&self.table_name)
                .filter_expression("begins_with(PK, :prefix)")
                .expression_attribute_values(":prefix", AttributeValue::S("TASK#".into()));

            if let Some(key) = exclusive_start_key.take() {
                req = req.set_exclusive_start_key(Some(key));
            }

            let resp = req.send().await.context("failed to scan tasks")?;

            for item in resp.items() {
                tasks.push(item_to_task(item)?);
            }

            match resp.last_evaluated_key() {
                Some(key) => exclusive_start_key = Some(key.to_owned()),
                None => break,
            }
        }

        tasks.sort_by_key(|t| t.id());
        Ok(tasks)
    }

    async fn get_task_internal(&self, id: i64) -> Result<Task> {
        let client = self.client().await?;
        let resp = client
            .get_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("TASK#{id}")))
            .key("SK", AttributeValue::S(format!("TASK#{id}")))
            .send()
            .await
            .context("failed to get task")?;

        let item = resp.item().context("task not found")?;
        item_to_task(item)
    }

    async fn batch_get_tasks(&self, ids: &[i64]) -> Result<Vec<Task>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let client = self.client().await?;
        let mut tasks = Vec::new();

        for chunk in ids.chunks(100) {
            let keys: Vec<HashMap<String, AttributeValue>> = chunk
                .iter()
                .map(|id| {
                    let mut key = HashMap::new();
                    key.insert("PK".into(), AttributeValue::S(format!("TASK#{id}")));
                    key.insert("SK".into(), AttributeValue::S(format!("TASK#{id}")));
                    key
                })
                .collect();

            let resp = client
                .batch_get_item()
                .request_items(
                    &self.table_name,
                    aws_sdk_dynamodb::types::KeysAndAttributes::builder()
                        .set_keys(Some(keys))
                        .build()?,
                )
                .send()
                .await
                .context("failed to batch get tasks")?;

            if let Some(responses) = resp.responses() {
                if let Some(items) = responses.get(&self.table_name) {
                    for item in items {
                        tasks.push(item_to_task(item)?);
                    }
                }
            }
        }

        Ok(tasks)
    }

    async fn get_ready_tasks(&self) -> Result<Vec<Task>> {
        let all = self.scan_all_tasks().await?;
        let todo_tasks: Vec<&Task> = all.iter().filter(|t| t.status() == TaskStatus::Todo).collect();

        if todo_tasks.is_empty() {
            return Ok(Vec::new());
        }

        let dep_ids: HashSet<i64> = todo_tasks
            .iter()
            .flat_map(|t| t.dependencies().iter().copied())
            .collect();
        let dep_tasks = self.batch_get_tasks(&dep_ids.into_iter().collect::<Vec<_>>()).await?;
        let dep_status: HashMap<i64, TaskStatus> = dep_tasks
            .into_iter()
            .map(|t| (t.id(), t.status()))
            .collect();

        let mut ready = Vec::new();
        for task in todo_tasks {
            let all_deps_completed = task.dependencies().iter().all(|dep_id| {
                dep_status.get(dep_id).map_or(true, |s| *s == TaskStatus::Completed)
            });
            if all_deps_completed {
                ready.push(task.clone());
            }
        }
        Ok(ready)
    }

    async fn scan_items_by_prefix(&self, prefix: &str) -> Result<Vec<HashMap<String, AttributeValue>>> {
        let client = self.client().await?;
        let mut items = Vec::new();
        let mut exclusive_start_key = None;

        loop {
            let mut req = client
                .scan()
                .table_name(&self.table_name)
                .filter_expression("begins_with(PK, :prefix)")
                .expression_attribute_values(":prefix", AttributeValue::S(prefix.into()));

            if let Some(key) = exclusive_start_key.take() {
                req = req.set_exclusive_start_key(Some(key));
            }

            let resp = req.send().await.context("failed to scan items")?;

            for item in resp.items() {
                items.push(item.clone());
            }

            match resp.last_evaluated_key() {
                Some(key) => exclusive_start_key = Some(key.to_owned()),
                None => break,
            }
        }

        Ok(items)
    }
}

// --- Attribute helpers ---

fn get_s(av: &AttributeValue) -> Option<&str> {
    av.as_s().ok().map(|s| s.as_str())
}

fn get_n(av: &AttributeValue) -> Result<i64> {
    let s = av.as_n().map_err(|_| anyhow::anyhow!("expected N attribute"))?;
    s.parse::<i64>().context("invalid number")
}

fn get_bool(av: &AttributeValue) -> Option<bool> {
    av.as_bool().ok().copied()
}

fn opt_s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key).and_then(|v| get_s(v).map(|s| s.to_string()))
}

fn req_s(item: &HashMap<String, AttributeValue>, key: &str) -> Result<String> {
    opt_s(item, key).with_context(|| format!("missing required field: {key}"))
}

fn opt_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key).and_then(|v| get_n(v).ok())
}

fn opt_s_list(item: &HashMap<String, AttributeValue>, key: &str) -> Vec<String> {
    item.get(key)
        .and_then(|v| v.as_l().ok())
        .map(|list| {
            list.iter()
                .filter_map(|v| get_s(v).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn opt_n_list(item: &HashMap<String, AttributeValue>, key: &str) -> Vec<i64> {
    item.get(key)
        .and_then(|v| v.as_l().ok())
        .map(|list| {
            list.iter()
                .filter_map(|v| get_n(v).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn opt_dod_list(item: &HashMap<String, AttributeValue>, key: &str) -> Vec<DodItem> {
    item.get(key)
        .and_then(|v| v.as_l().ok())
        .map(|list| {
            list.iter()
                .filter_map(|v| {
                    let m = v.as_m().ok()?;
                    let content = opt_s(m, "content")?;
                    let checked = m.get("checked").and_then(|v| get_bool(v)).unwrap_or(false);
                    Some(DodItem::new(content, checked))
                })
                .collect()
        })
        .unwrap_or_default()
}

// --- Conversion helpers ---

fn task_to_item(task: &Task) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    let pk = format!("TASK#{}", task.id());
    item.insert("PK".into(), AttributeValue::S(pk.clone()));
    item.insert("SK".into(), AttributeValue::S(pk));
    item.insert("id".into(), AttributeValue::N(task.id().to_string()));
    item.insert("project_id".into(), AttributeValue::N(task.project_id().to_string()));
    item.insert("title".into(), AttributeValue::S(task.title().to_string()));
    item.insert("status".into(), AttributeValue::S(task.status().to_string()));
    item.insert("priority".into(), AttributeValue::N(i32::from(task.priority()).to_string()));
    item.insert("created_at".into(), AttributeValue::S(task.created_at().to_string()));
    item.insert("updated_at".into(), AttributeValue::S(task.updated_at().to_string()));

    if let Some(v) = task.background() {
        item.insert("background".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.description() {
        item.insert("description".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.plan() {
        item.insert("plan".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.assignee_session_id() {
        item.insert("assignee_session_id".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.assignee_user_id() {
        item.insert("assignee_user_id".into(), AttributeValue::N(v.to_string()));
    }
    if let Some(v) = task.started_at() {
        item.insert("started_at".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.completed_at() {
        item.insert("completed_at".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.canceled_at() {
        item.insert("canceled_at".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.cancel_reason() {
        item.insert("cancel_reason".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.branch() {
        item.insert("branch".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.pr_url() {
        item.insert("pr_url".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = task.metadata() {
        item.insert("metadata".into(), AttributeValue::S(serde_json::to_string(v).unwrap()));
    }

    // Lists
    item.insert(
        "tags".into(),
        AttributeValue::L(task.tags().iter().map(|t| AttributeValue::S(t.clone())).collect()),
    );
    item.insert(
        "dependencies".into(),
        AttributeValue::L(
            task.dependencies()
                .iter()
                .map(|d| AttributeValue::N(d.to_string()))
                .collect(),
        ),
    );
    item.insert(
        "definition_of_done".into(),
        AttributeValue::L(
            task.definition_of_done()
                .iter()
                .map(|d| {
                    let mut m = HashMap::new();
                    m.insert("content".into(), AttributeValue::S(d.content().to_string()));
                    m.insert("checked".into(), AttributeValue::Bool(d.checked()));
                    AttributeValue::M(m)
                })
                .collect(),
        ),
    );
    item.insert(
        "in_scope".into(),
        AttributeValue::L(task.in_scope().iter().map(|s| AttributeValue::S(s.clone())).collect()),
    );
    item.insert(
        "out_of_scope".into(),
        AttributeValue::L(
            task.out_of_scope()
                .iter()
                .map(|s| AttributeValue::S(s.clone()))
                .collect(),
        ),
    );

    item
}

fn item_to_task(item: &HashMap<String, AttributeValue>) -> Result<Task> {
    let id = item
        .get("id")
        .and_then(|v| get_n(v).ok())
        .context("missing id")?;
    let project_id = item
        .get("project_id")
        .and_then(|v| get_n(v).ok())
        .unwrap_or(1); // default for legacy items
    let title = req_s(item, "title")?;
    let status_str = req_s(item, "status")?;
    let status: TaskStatus = status_str.parse()?;
    let priority_val = item
        .get("priority")
        .and_then(|v| get_n(v).ok())
        .unwrap_or(2) as i32;
    let priority = Priority::try_from(priority_val)?;
    let created_at = req_s(item, "created_at")?;
    let updated_at = req_s(item, "updated_at")?;

    let metadata_str = opt_s(item, "metadata");
    let metadata: Option<serde_json::Value> = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("invalid metadata JSON")?;

    Ok(Task::new(
        id,
        project_id,
        title,
        opt_s(item, "background"),
        opt_s(item, "description"),
        opt_s(item, "plan"),
        priority,
        status,
        opt_s(item, "assignee_session_id"),
        opt_n(item, "assignee_user_id"),
        created_at,
        updated_at,
        opt_s(item, "started_at"),
        opt_s(item, "completed_at"),
        opt_s(item, "canceled_at"),
        opt_s(item, "cancel_reason"),
        opt_s(item, "branch"),
        opt_s(item, "pr_url"),
        metadata,
        opt_dod_list(item, "definition_of_done"),
        opt_s_list(item, "in_scope"),
        opt_s_list(item, "out_of_scope"),
        opt_s_list(item, "tags"),
        opt_n_list(item, "dependencies"),
    ))
}

fn project_to_item(project: &Project) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    let pk = format!("PROJECT#{}", project.id());
    item.insert("PK".into(), AttributeValue::S(pk.clone()));
    item.insert("SK".into(), AttributeValue::S(pk));
    item.insert("id".into(), AttributeValue::N(project.id().to_string()));
    item.insert("name".into(), AttributeValue::S(project.name().to_string()));
    if let Some(desc) = project.description() {
        item.insert("description".into(), AttributeValue::S(desc.to_string()));
    }
    item.insert("created_at".into(), AttributeValue::S(project.created_at().to_string()));
    item
}

fn item_to_project(item: &HashMap<String, AttributeValue>) -> Result<Project> {
    Ok(Project::new(
        opt_n(item, "id").context("missing id")?,
        req_s(item, "name")?,
        opt_s(item, "description"),
        req_s(item, "created_at")?,
    ))
}

fn user_to_item(user: &User) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    let pk = format!("USER#{}", user.id());
    item.insert("PK".into(), AttributeValue::S(pk.clone()));
    item.insert("SK".into(), AttributeValue::S(pk));
    item.insert("id".into(), AttributeValue::N(user.id().to_string()));
    item.insert("username".into(), AttributeValue::S(user.username().to_string()));
    if let Some(v) = user.display_name() {
        item.insert("display_name".into(), AttributeValue::S(v.to_string()));
    }
    if let Some(v) = user.email() {
        item.insert("email".into(), AttributeValue::S(v.to_string()));
    }
    item.insert("created_at".into(), AttributeValue::S(user.created_at().to_string()));
    item
}

fn item_to_user(item: &HashMap<String, AttributeValue>) -> Result<User> {
    Ok(User::new(
        opt_n(item, "id").context("missing id")?,
        req_s(item, "username")?,
        opt_s(item, "display_name"),
        opt_s(item, "email"),
        req_s(item, "created_at")?,
    ))
}

fn member_to_item(member: &ProjectMember) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    let pk = format!("PROJMEMBER#{}#{}", member.project_id(), member.user_id());
    item.insert("PK".into(), AttributeValue::S(pk.clone()));
    item.insert("SK".into(), AttributeValue::S(pk));
    item.insert("id".into(), AttributeValue::N(member.id().to_string()));
    item.insert("project_id".into(), AttributeValue::N(member.project_id().to_string()));
    item.insert("user_id".into(), AttributeValue::N(member.user_id().to_string()));
    item.insert("role".into(), AttributeValue::S(member.role().to_string()));
    item.insert("created_at".into(), AttributeValue::S(member.created_at().to_string()));
    item
}

fn item_to_member(item: &HashMap<String, AttributeValue>) -> Result<ProjectMember> {
    let role_str = req_s(item, "role")?;
    let role: Role = role_str.parse()?;
    Ok(ProjectMember::new(
        opt_n(item, "id").context("missing id")?,
        opt_n(item, "project_id").context("missing project_id")?,
        opt_n(item, "user_id").context("missing user_id")?,
        role,
        req_s(item, "created_at")?,
    ))
}

#[async_trait]
impl ProjectRepository for DynamoDbBackend {
    // Project management

    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        let id = self.next_id("PROJECT").await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let project = Project::new(
            id,
            params.name.clone(),
            params.description.clone(),
            now,
        );
        self.put_item(project_to_item(&project)).await?;
        Ok(project)
    }

    async fn get_project(&self, id: i64) -> Result<Project> {
        let client = self.client().await?;
        let resp = client
            .get_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("PROJECT#{id}")))
            .key("SK", AttributeValue::S(format!("PROJECT#{id}")))
            .send()
            .await
            .context("failed to get project")?;
        let item = resp.item().context("project not found")?;
        item_to_project(item)
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let projects = self.list_projects().await?;
        projects
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| anyhow::anyhow!("project not found"))
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let items = self.scan_items_by_prefix("PROJECT#").await?;
        let mut projects: Vec<Project> = items.iter().map(|i| item_to_project(i)).collect::<Result<_>>()?;
        projects.sort_by_key(|p| p.id());
        Ok(projects)
    }

    async fn delete_project(&self, id: i64) -> Result<()> {
        let client = self.client().await?;
        client
            .delete_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("PROJECT#{id}")))
            .key("SK", AttributeValue::S(format!("PROJECT#{id}")))
            .send()
            .await
            .context("failed to delete project")?;
        Ok(())
    }

    async fn add_project_member(&self, project_id: i64, params: &AddProjectMemberParams) -> Result<ProjectMember> {
        let id = self.next_id("MEMBER").await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let member = ProjectMember::new(
            id,
            project_id,
            params.user_id,
            params.role,
            now,
        );
        self.put_item(member_to_item(&member)).await?;
        Ok(member)
    }

    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()> {
        let client = self.client().await?;
        let pk = format!("PROJMEMBER#{project_id}#{user_id}");
        client
            .delete_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(pk.clone()))
            .key("SK", AttributeValue::S(pk))
            .send()
            .await
            .context("failed to remove project member")?;
        Ok(())
    }

    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>> {
        let prefix = format!("PROJMEMBER#{project_id}#");
        let items = self.scan_items_by_prefix(&prefix).await?;
        let mut members: Vec<ProjectMember> = items.iter().map(|i| item_to_member(i)).collect::<Result<_>>()?;
        members.sort_by_key(|m| m.id());
        Ok(members)
    }

    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember> {
        let client = self.client().await?;
        let pk = format!("PROJMEMBER#{project_id}#{user_id}");
        let resp = client
            .get_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(pk.clone()))
            .key("SK", AttributeValue::S(pk))
            .send()
            .await
            .context("failed to get project member")?;
        let item = resp.item().context("project member not found")?;
        item_to_member(item)
    }

    async fn update_member_role(&self, project_id: i64, user_id: i64, role: Role) -> Result<ProjectMember> {
        let member = self.get_project_member(project_id, user_id).await?;
        let member = ProjectMember::new(member.id(), member.project_id(), member.user_id(), role, member.created_at().to_string());
        self.put_item(member_to_item(&member)).await?;
        Ok(member)
    }
}

#[async_trait]
impl UserRepository for DynamoDbBackend {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let id = self.next_id("USER").await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let user = User::new(
            id,
            params.username.clone(),
            params.display_name.clone(),
            params.email.clone(),
            now,
        );
        self.put_item(user_to_item(&user)).await?;
        Ok(user)
    }

    async fn get_user(&self, id: i64) -> Result<User> {
        let client = self.client().await?;
        let resp = client
            .get_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("USER#{id}")))
            .key("SK", AttributeValue::S(format!("USER#{id}")))
            .send()
            .await
            .context("failed to get user")?;
        let item = resp.item().context("user not found")?;
        item_to_user(item)
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let users = self.list_users().await?;
        users
            .into_iter()
            .find(|u| u.username() == username)
            .ok_or_else(|| anyhow::anyhow!("user not found"))
    }

    async fn list_users(&self) -> Result<Vec<User>> {
        let items = self.scan_items_by_prefix("USER#").await?;
        let mut users: Vec<User> = items.iter().map(|i| item_to_user(i)).collect::<Result<_>>()?;
        users.sort_by_key(|u| u.id());
        Ok(users)
    }

    async fn delete_user(&self, id: i64) -> Result<()> {
        let client = self.client().await?;
        client
            .delete_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("USER#{id}")))
            .key("SK", AttributeValue::S(format!("USER#{id}")))
            .send()
            .await
            .context("failed to delete user")?;
        Ok(())
    }
}

#[async_trait]
impl ApiKeyRepository for DynamoDbBackend {
    fn supports_api_key_management(&self) -> bool {
        false
    }

    fn supports_api_key_auth(&self) -> bool {
        false
    }

    async fn create_api_key(&self, _user_id: i64, _name: &str, _new_key: &NewApiKey) -> Result<ApiKeyWithSecret> {
        Err(DomainError::UnsupportedOperation {
            operation: "create_api_key".into(),
        }.into())
    }

    async fn get_user_by_api_key(&self, _key_hash: &str) -> Result<User> {
        Err(DomainError::UnsupportedOperation {
            operation: "get_user_by_api_key".into(),
        }.into())
    }

    async fn list_api_keys(&self, _user_id: i64) -> Result<Vec<ApiKey>> {
        Err(DomainError::UnsupportedOperation {
            operation: "list_api_keys".into(),
        }.into())
    }

    async fn delete_api_key(&self, _key_id: i64) -> Result<()> {
        Err(DomainError::UnsupportedOperation {
            operation: "delete_api_key".into(),
        }.into())
    }
}

#[async_trait]
impl TaskRepository for DynamoDbBackend {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
        let id = self.next_id("TASK").await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let priority = params.priority.unwrap_or(Priority::P2);

        let task = Task::new(
            id,
            project_id,
            params.title.clone(),
            params.background.clone(),
            params.description.clone(),
            None,
            priority,
            TaskStatus::Draft,
            None,
            None,
            now.clone(),
            now,
            None,
            None,
            None,
            None,
            params.branch.clone(),
            params.pr_url.clone(),
            params.metadata.clone(),
            params
                .definition_of_done
                .iter()
                .map(|c| DodItem::new(c.clone(), false))
                .collect(),
            params.in_scope.clone(),
            params.out_of_scope.clone(),
            params.tags.clone(),
            params.dependencies.clone(),
        );

        self.put_task(&task).await?;
        Ok(task)
    }

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let task = self.get_task_internal(id).await?;
        if task.project_id() != project_id {
            bail!("task {id} does not belong to project {project_id}");
        }
        Ok(task)
    }

    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let task = task.apply_update(params, now);
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let task = task.apply_array_update(params, now);
        self.put_task(&task).await?;
        Ok(())
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        // Verify project ownership first
        let _ = self.get_task(project_id, id).await?;
        let client = self.client().await?;
        client
            .delete_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("TASK#{id}")))
            .key("SK", AttributeValue::S(format!("TASK#{id}")))
            .send()
            .await
            .context("failed to delete task")?;
        Ok(())
    }

    async fn add_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task> {
        let task = self.get_task(project_id, task_id).await?;
        let _ = self.get_task_internal(dep_id).await.context("dependency task not found")?;

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.add_dependency(dep_id, Some(now))?;
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn remove_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task> {
        let task = self.get_task(project_id, task_id).await?;

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.remove_dependency(dep_id, Some(now))?;
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn set_dependencies(&self, project_id: i64, task_id: i64, dep_ids: &[i64]) -> Result<Task> {
        let task = self.get_task(project_id, task_id).await?;

        for &dep_id in dep_ids {
            let _ = self.get_task_internal(dep_id).await.context("dependency task not found")?;
        }

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.set_dependencies(dep_ids, Some(now))?;
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>> {
        let task = self.get_task(project_id, task_id).await?;
        self.batch_get_tasks(task.dependencies()).await
    }

    async fn save(&self, task: &Task) -> Result<()> {
        self.put_task(task).await
    }
}

#[async_trait]
impl TaskQueryPort for DynamoDbBackend {
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>> {
        let all = self.scan_all_tasks().await?;
        let mut result: Vec<Task> = all
            .into_iter()
            .filter(|task| {
                if task.project_id() != project_id {
                    return false;
                }
                if !filter.statuses.is_empty() && !filter.statuses.contains(&task.status()) {
                    return false;
                }
                if !filter.tags.is_empty()
                    && !filter.tags.iter().any(|t| task.tags().contains(t))
                {
                    return false;
                }
                if let Some(dep_id) = filter.depends_on {
                    if !task.dependencies().contains(&dep_id) {
                        return false;
                    }
                }
                if filter.ready && task.status() != TaskStatus::Todo {
                    return false;
                }
                true
            })
            .collect();

        if filter.ready && !result.is_empty() {
            let dep_ids: HashSet<i64> = result
                .iter()
                .flat_map(|t| t.dependencies().iter().copied())
                .collect();
            let dep_tasks = self
                .batch_get_tasks(&dep_ids.into_iter().collect::<Vec<_>>())
                .await?;
            let dep_statuses: HashMap<i64, TaskStatus> =
                dep_tasks.into_iter().map(|t| (t.id(), t.status())).collect();

            result = crate::domain::task::filter_ready(result, &dep_statuses);
        }

        result.sort_by_key(|t| t.id());
        Ok(result)
    }

    async fn next_task(&self, project_id: i64) -> Result<Option<Task>> {
        let all = self.scan_all_tasks().await?;
        let todo_tasks: Vec<Task> = all
            .into_iter()
            .filter(|t| t.project_id() == project_id && t.status() == TaskStatus::Todo)
            .collect();

        if todo_tasks.is_empty() {
            return Ok(None);
        }

        let dep_ids: HashSet<i64> = todo_tasks
            .iter()
            .flat_map(|t| t.dependencies().iter().copied())
            .collect();
        let dep_tasks = self.batch_get_tasks(&dep_ids.into_iter().collect::<Vec<_>>()).await?;
        let dep_statuses: HashMap<i64, TaskStatus> =
            dep_tasks.into_iter().map(|t| (t.id(), t.status())).collect();

        Ok(crate::domain::task::select_next(todo_tasks, &dep_statuses))
    }

    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        let all = self.scan_all_tasks().await?;
        let mut stats = HashMap::new();
        for task in all.iter().filter(|t| t.project_id() == project_id) {
            *stats.entry(task.status().to_string()).or_insert(0) += 1;
        }
        Ok(stats)
    }

    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        let ready = self.list_tasks(project_id, &ListTasksFilter {
            ready: true,
            ..Default::default()
        }).await?;
        Ok(ready.len() as i64)
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        self.list_tasks(project_id, &ListTasksFilter {
            ready: true,
            ..Default::default()
        }).await
    }
}
