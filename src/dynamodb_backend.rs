use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use chrono::Utc;
use tokio::sync::OnceCell;

use crate::backend::TaskBackend;
use crate::models::{
    CreateTaskParams, DodItem, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
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

    async fn next_id(&self) -> Result<i64> {
        let client = self.client().await?;
        let resp = client
            .update_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S("COUNTER".into()))
            .key("SK", AttributeValue::S("COUNTER".into()))
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

        tasks.sort_by_key(|t| t.id);
        Ok(tasks)
    }

    async fn has_cycle(&self, task_id: i64, dep_id: i64) -> Result<bool> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(dep_id);
        visited.insert(dep_id);

        while let Some(current) = queue.pop_front() {
            let task = match self.get_task(current).await {
                Ok(t) => t,
                Err(_) => continue,
            };
            for &d in &task.dependencies {
                if d == task_id {
                    return Ok(true);
                }
                if visited.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        Ok(false)
    }

    async fn is_task_ready(&self, task: &Task) -> Result<bool> {
        if task.status != TaskStatus::Todo {
            return Ok(false);
        }
        if task.dependencies.is_empty() {
            return Ok(true);
        }
        let dep_tasks = self.batch_get_tasks(&task.dependencies).await?;
        for dep in &dep_tasks {
            if dep.status != TaskStatus::Completed {
                return Ok(false);
            }
        }
        // If a dependency was deleted, treat as non-blocking
        Ok(true)
    }

    async fn batch_get_tasks(&self, ids: &[i64]) -> Result<Vec<Task>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let client = self.client().await?;
        let mut tasks = Vec::new();

        // BatchGetItem supports max 100 keys per request
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
        let todo_tasks: Vec<&Task> = all.iter().filter(|t| t.status == TaskStatus::Todo).collect();

        if todo_tasks.is_empty() {
            return Ok(Vec::new());
        }

        // Collect all dependency IDs
        let dep_ids: HashSet<i64> = todo_tasks
            .iter()
            .flat_map(|t| t.dependencies.iter().copied())
            .collect();
        let dep_tasks = self.batch_get_tasks(&dep_ids.into_iter().collect::<Vec<_>>()).await?;
        let dep_status: HashMap<i64, TaskStatus> = dep_tasks
            .into_iter()
            .map(|t| (t.id, t.status))
            .collect();

        let mut ready = Vec::new();
        for task in todo_tasks {
            let all_deps_completed = task.dependencies.iter().all(|dep_id| {
                dep_status.get(dep_id).map_or(true, |s| *s == TaskStatus::Completed)
            });
            if all_deps_completed {
                ready.push(task.clone());
            }
        }
        Ok(ready)
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
                    Some(DodItem { content, checked })
                })
                .collect()
        })
        .unwrap_or_default()
}

// --- Conversion helpers ---

fn task_to_item(task: &Task) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    let pk = format!("TASK#{}", task.id);
    item.insert("PK".into(), AttributeValue::S(pk.clone()));
    item.insert("SK".into(), AttributeValue::S(pk));
    item.insert("id".into(), AttributeValue::N(task.id.to_string()));
    item.insert("title".into(), AttributeValue::S(task.title.clone()));
    item.insert("status".into(), AttributeValue::S(task.status.to_string()));
    item.insert("priority".into(), AttributeValue::N(i32::from(task.priority).to_string()));
    item.insert("created_at".into(), AttributeValue::S(task.created_at.clone()));
    item.insert("updated_at".into(), AttributeValue::S(task.updated_at.clone()));

    if let Some(ref v) = task.background {
        item.insert("background".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.description {
        item.insert("description".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.plan {
        item.insert("plan".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.assignee_session_id {
        item.insert("assignee_session_id".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.started_at {
        item.insert("started_at".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.completed_at {
        item.insert("completed_at".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.canceled_at {
        item.insert("canceled_at".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.cancel_reason {
        item.insert("cancel_reason".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.branch {
        item.insert("branch".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.pr_url {
        item.insert("pr_url".into(), AttributeValue::S(v.clone()));
    }
    if let Some(ref v) = task.metadata {
        item.insert("metadata".into(), AttributeValue::S(serde_json::to_string(v).unwrap()));
    }

    // Lists
    item.insert(
        "tags".into(),
        AttributeValue::L(task.tags.iter().map(|t| AttributeValue::S(t.clone())).collect()),
    );
    item.insert(
        "dependencies".into(),
        AttributeValue::L(
            task.dependencies
                .iter()
                .map(|d| AttributeValue::N(d.to_string()))
                .collect(),
        ),
    );
    item.insert(
        "definition_of_done".into(),
        AttributeValue::L(
            task.definition_of_done
                .iter()
                .map(|d| {
                    let mut m = HashMap::new();
                    m.insert("content".into(), AttributeValue::S(d.content.clone()));
                    m.insert("checked".into(), AttributeValue::Bool(d.checked));
                    AttributeValue::M(m)
                })
                .collect(),
        ),
    );
    item.insert(
        "in_scope".into(),
        AttributeValue::L(task.in_scope.iter().map(|s| AttributeValue::S(s.clone())).collect()),
    );
    item.insert(
        "out_of_scope".into(),
        AttributeValue::L(
            task.out_of_scope
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

    Ok(Task {
        id,
        title,
        background: opt_s(item, "background"),
        description: opt_s(item, "description"),
        plan: opt_s(item, "plan"),
        priority,
        status,
        assignee_session_id: opt_s(item, "assignee_session_id"),
        created_at,
        updated_at,
        started_at: opt_s(item, "started_at"),
        completed_at: opt_s(item, "completed_at"),
        canceled_at: opt_s(item, "canceled_at"),
        cancel_reason: opt_s(item, "cancel_reason"),
        branch: opt_s(item, "branch"),
        pr_url: opt_s(item, "pr_url"),
        metadata,
        definition_of_done: opt_dod_list(item, "definition_of_done"),
        in_scope: opt_s_list(item, "in_scope"),
        out_of_scope: opt_s_list(item, "out_of_scope"),
        tags: opt_s_list(item, "tags"),
        dependencies: opt_n_list(item, "dependencies"),
    })
}

#[async_trait]
impl TaskBackend for DynamoDbBackend {
    async fn create_task(&self, params: &CreateTaskParams) -> Result<Task> {
        let id = self.next_id().await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let priority = params.priority.unwrap_or(Priority::P2);

        let task = Task {
            id,
            title: params.title.clone(),
            background: params.background.clone(),
            description: params.description.clone(),
            plan: None,
            priority,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: now.clone(),
            updated_at: now,
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: params.branch.clone(),
            pr_url: params.pr_url.clone(),
            metadata: params.metadata.clone(),
            definition_of_done: params
                .definition_of_done
                .iter()
                .map(|c| DodItem {
                    content: c.clone(),
                    checked: false,
                })
                .collect(),
            in_scope: params.in_scope.clone(),
            out_of_scope: params.out_of_scope.clone(),
            tags: params.tags.clone(),
            dependencies: params.dependencies.clone(),
        };

        self.put_task(&task).await?;
        Ok(task)
    }

    async fn get_task(&self, id: i64) -> Result<Task> {
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

    async fn ready_task(&self, id: i64) -> Result<Task> {
        let mut task = self.get_task(id).await?;
        task.status.transition_to(TaskStatus::Todo)?;
        task.status = TaskStatus::Todo;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn start_task(
        &self,
        id: i64,
        assignee_session_id: Option<String>,
        started_at: &str,
    ) -> Result<Task> {
        let mut task = self.get_task(id).await?;
        task.status.transition_to(TaskStatus::InProgress)?;
        task.status = TaskStatus::InProgress;
        task.assignee_session_id = assignee_session_id;
        task.started_at = Some(started_at.to_string());
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn complete_task(&self, id: i64, completed_at: &str) -> Result<Task> {
        let mut task = self.get_task(id).await?;
        task.status.transition_to(TaskStatus::Completed)?;
        task.status = TaskStatus::Completed;
        task.completed_at = Some(completed_at.to_string());
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn cancel_task(
        &self,
        id: i64,
        canceled_at: &str,
        reason: Option<String>,
    ) -> Result<Task> {
        let mut task = self.get_task(id).await?;
        task.status.transition_to(TaskStatus::Canceled)?;
        task.status = TaskStatus::Canceled;
        task.canceled_at = Some(canceled_at.to_string());
        task.cancel_reason = reason;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn update_task(&self, id: i64, params: &UpdateTaskParams) -> Result<Task> {
        let mut task = self.get_task(id).await?;
        let mut changed = false;

        if let Some(ref title) = params.title {
            task.title = title.clone();
            changed = true;
        }
        if let Some(ref v) = params.background {
            task.background = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.description {
            task.description = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.plan {
            task.plan = v.clone();
            changed = true;
        }
        if let Some(p) = params.priority {
            task.priority = p;
            changed = true;
        }
        if let Some(ref v) = params.assignee_session_id {
            task.assignee_session_id = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.started_at {
            task.started_at = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.completed_at {
            task.completed_at = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.canceled_at {
            task.canceled_at = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.cancel_reason {
            task.cancel_reason = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.branch {
            task.branch = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.pr_url {
            task.pr_url = v.clone();
            changed = true;
        }
        if let Some(ref v) = params.metadata {
            task.metadata = v.clone();
            changed = true;
        }

        if changed {
            task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            self.put_task(&task).await?;
        }

        Ok(task)
    }

    async fn update_task_arrays(&self, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
        let mut task = self.get_task(id).await?;
        let mut changed = false;

        // Tags
        if let Some(ref set_tags) = params.set_tags {
            task.tags = set_tags.clone();
            changed = true;
        }
        if !params.add_tags.is_empty() {
            for tag in &params.add_tags {
                if !task.tags.contains(tag) {
                    task.tags.push(tag.clone());
                }
            }
            changed = true;
        }
        if !params.remove_tags.is_empty() {
            task.tags.retain(|t| !params.remove_tags.contains(t));
            changed = true;
        }

        // Definition of Done
        if let Some(ref set_dod) = params.set_definition_of_done {
            task.definition_of_done = set_dod
                .iter()
                .map(|c| DodItem {
                    content: c.clone(),
                    checked: false,
                })
                .collect();
            changed = true;
        }
        if !params.add_definition_of_done.is_empty() {
            for item in &params.add_definition_of_done {
                task.definition_of_done.push(DodItem {
                    content: item.clone(),
                    checked: false,
                });
            }
            changed = true;
        }
        if !params.remove_definition_of_done.is_empty() {
            task.definition_of_done
                .retain(|d| !params.remove_definition_of_done.contains(&d.content));
            changed = true;
        }

        // In Scope
        if let Some(ref set_scope) = params.set_in_scope {
            task.in_scope = set_scope.clone();
            changed = true;
        }
        if !params.add_in_scope.is_empty() {
            task.in_scope.extend(params.add_in_scope.clone());
            changed = true;
        }
        if !params.remove_in_scope.is_empty() {
            task.in_scope.retain(|s| !params.remove_in_scope.contains(s));
            changed = true;
        }

        // Out of Scope
        if let Some(ref set_scope) = params.set_out_of_scope {
            task.out_of_scope = set_scope.clone();
            changed = true;
        }
        if !params.add_out_of_scope.is_empty() {
            task.out_of_scope.extend(params.add_out_of_scope.clone());
            changed = true;
        }
        if !params.remove_out_of_scope.is_empty() {
            task.out_of_scope
                .retain(|s| !params.remove_out_of_scope.contains(s));
            changed = true;
        }

        if changed {
            task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            self.put_task(&task).await?;
        }

        Ok(())
    }

    async fn delete_task(&self, id: i64) -> Result<()> {
        let client = self.client().await?;
        let resp = client
            .delete_item()
            .table_name(&self.table_name)
            .key("PK", AttributeValue::S(format!("TASK#{id}")))
            .key("SK", AttributeValue::S(format!("TASK#{id}")))
            .condition_expression("attribute_exists(PK)")
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllOld)
            .send()
            .await;

        match resp {
            Ok(_) => Ok(()),
            Err(err) => {
                let is_condition_fail = err
                    .as_service_error()
                    .map_or(false, |e| e.is_conditional_check_failed_exception());
                if is_condition_fail {
                    bail!("task not found");
                }
                Err(anyhow::anyhow!("failed to delete task: {err}"))
            }
        }
    }

    async fn list_tasks(&self, filter: &ListTasksFilter) -> Result<Vec<Task>> {
        let all = self.scan_all_tasks().await?;
        let mut result: Vec<Task> = all
            .into_iter()
            .filter(|task| {
                // Status filter
                if !filter.statuses.is_empty() && !filter.statuses.contains(&task.status) {
                    return false;
                }

                // Tag filter (any match)
                if !filter.tags.is_empty()
                    && !filter.tags.iter().any(|t| task.tags.contains(t))
                {
                    return false;
                }

                // depends_on filter
                if let Some(dep_id) = filter.depends_on {
                    if !task.dependencies.contains(&dep_id) {
                        return false;
                    }
                }

                // ready filter (status check only; dep check below)
                if filter.ready && task.status != TaskStatus::Todo {
                    return false;
                }

                true
            })
            .collect();

        // For ready filter, check dependencies asynchronously
        if filter.ready && !result.is_empty() {
            let dep_ids: HashSet<i64> = result
                .iter()
                .flat_map(|t| t.dependencies.iter().copied())
                .collect();
            let dep_tasks = self
                .batch_get_tasks(&dep_ids.into_iter().collect::<Vec<_>>())
                .await?;
            let dep_status: HashMap<i64, TaskStatus> =
                dep_tasks.into_iter().map(|t| (t.id, t.status)).collect();

            result.retain(|task| {
                task.dependencies.iter().all(|dep_id| {
                    dep_status
                        .get(dep_id)
                        .map_or(true, |s| *s == TaskStatus::Completed)
                })
            });
        }

        result.sort_by_key(|t| t.id);
        Ok(result)
    }

    async fn next_task(&self) -> Result<Option<Task>> {
        let ready = self.get_ready_tasks().await?;
        if ready.is_empty() {
            return Ok(None);
        }

        let mut sorted = ready;
        sorted.sort_by(|a, b| {
            let pa = i32::from(a.priority);
            let pb = i32::from(b.priority);
            pa.cmp(&pb)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok(sorted.into_iter().next())
    }

    async fn task_stats(&self) -> Result<HashMap<String, i64>> {
        let all = self.scan_all_tasks().await?;
        let mut stats = HashMap::new();
        for task in &all {
            *stats.entry(task.status.to_string()).or_insert(0) += 1;
        }
        Ok(stats)
    }

    async fn ready_count(&self) -> Result<i64> {
        let ready = self.get_ready_tasks().await?;
        Ok(ready.len() as i64)
    }

    async fn list_ready_tasks(&self) -> Result<Vec<Task>> {
        self.get_ready_tasks().await
    }

    async fn add_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task> {
        if task_id == dep_id {
            bail!("a task cannot depend on itself");
        }

        let mut task = self.get_task(task_id).await?;
        // Verify dependency exists
        let _ = self.get_task(dep_id).await.context("dependency task not found")?;

        if task.dependencies.contains(&dep_id) {
            return Ok(task);
        }

        if self.has_cycle(task_id, dep_id).await? {
            bail!("adding this dependency would create a cycle");
        }

        task.dependencies.push(dep_id);
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn remove_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task> {
        let mut task = self.get_task(task_id).await?;

        if !task.dependencies.contains(&dep_id) {
            bail!("dependency not found");
        }

        task.dependencies.retain(|&d| d != dep_id);
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn set_dependencies(&self, task_id: i64, dep_ids: &[i64]) -> Result<Task> {
        if dep_ids.contains(&task_id) {
            bail!("a task cannot depend on itself");
        }

        let mut task = self.get_task(task_id).await?;

        // Verify all dependencies exist
        for &dep_id in dep_ids {
            let _ = self.get_task(dep_id).await.context("dependency task not found")?;
        }

        // Check for cycles with new dependencies
        let old_deps = task.dependencies.clone();
        task.dependencies.clear();
        self.put_task(&task).await?;

        for &dep_id in dep_ids {
            if self.has_cycle(task_id, dep_id).await? {
                // Restore old dependencies on failure
                task.dependencies = old_deps;
                self.put_task(&task).await?;
                bail!("adding this dependency would create a cycle");
            }
            task.dependencies.push(dep_id);
            self.put_task(&task).await?;
        }

        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn list_dependencies(&self, task_id: i64) -> Result<Vec<Task>> {
        let task = self.get_task(task_id).await?;
        self.batch_get_tasks(&task.dependencies).await
    }

    async fn check_dod(&self, task_id: i64, index: usize) -> Result<Task> {
        let mut task = self.get_task(task_id).await?;
        if index == 0 || index > task.definition_of_done.len() {
            bail!(
                "DoD index out of range: {} (task has {} items)",
                index,
                task.definition_of_done.len()
            );
        }
        task.definition_of_done[index - 1].checked = true;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }

    async fn uncheck_dod(&self, task_id: i64, index: usize) -> Result<Task> {
        let mut task = self.get_task(task_id).await?;
        if index == 0 || index > task.definition_of_done.len() {
            bail!(
                "DoD index out of range: {} (task has {} items)",
                index,
                task.definition_of_done.len()
            );
        }
        task.definition_of_done[index - 1].checked = false;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.put_task(&task).await?;
        Ok(task)
    }
}
