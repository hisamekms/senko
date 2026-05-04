use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::contract::ContractId;
use super::error::DomainError;
use super::project::ProjectId;
use super::user::UserId;

/// Newtype wrapper around the per-project task identifier.
///
/// Wraps `i64` with `#[serde(transparent)]` so that the JSON wire format stays a
/// bare integer (e.g. `42`), not `{"0": 42}`. The goal is compile-time safety: a
/// `TaskId` cannot be accidentally mixed with a `ProjectId`, `user_id`, or
/// `contract_id` that also happen to be `i64`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Ord,
    PartialOrd,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(transparent)]
#[schema(value_type = i64)]
pub struct TaskId(pub i64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TaskId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i64>().map(TaskId)
    }
}

impl From<i64> for TaskId {
    fn from(n: i64) -> Self {
        TaskId(n)
    }
}

impl From<TaskId> for i64 {
    fn from(id: TaskId) -> i64 {
        id.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MergeVia {
    #[serde(alias = "merge_then_complete")]
    #[default]
    Direct,
    #[serde(alias = "pr_then_complete")]
    Pr,
}

impl fmt::Display for MergeVia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeVia::Direct => write!(f, "direct"),
            MergeVia::Pr => write!(f, "pr"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BranchMode {
    #[default]
    Worktree,
    Branch,
}

impl fmt::Display for BranchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BranchMode::Worktree => write!(f, "worktree"),
            BranchMode::Branch => write!(f, "branch"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MergeStrategy {
    #[default]
    Rebase,
    Squash,
}

impl fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeStrategy::Rebase => write!(f, "rebase"),
            MergeStrategy::Squash => write!(f, "squash"),
        }
    }
}

/// Domain event emitted by Task aggregate methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskEvent {
    Created,
    Published,
    Started,
    Resumed,
    Updated { changed_fields: Vec<String> },
    Completed,
    Canceled,
    DependencyAdded { dep_id: TaskId },
    DependencyRemoved { dep_id: TaskId },
    DependenciesSet { dep_ids: Vec<TaskId> },
    DodChecked { index: usize },
    DodUnchecked { index: usize },
}

/// A task that became eligible (ready) after another task was completed.
#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    id: TaskId,
    title: String,
    priority: Priority,
    metadata: Option<serde_json::Value>,
}

impl UnblockedTask {
    pub fn new(
        id: TaskId,
        title: String,
        priority: Priority,
        metadata: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id,
            title,
            priority,
            metadata,
        }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Draft,
    Todo,
    InProgress,
    Completed,
    Canceled,
}

impl TaskStatus {
    pub fn can_transition_to(&self, to: TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, to),
            (Draft, Todo)
                | (Todo, InProgress)
                | (InProgress, Completed)
                | (Draft, Canceled)
                | (Todo, Canceled)
                | (InProgress, Canceled)
        )
    }

    pub fn transition_to(&self, to: TaskStatus) -> anyhow::Result<TaskStatus> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(DomainError::InvalidStatusTransition {
                from: self.to_string(),
                to: to.to_string(),
            }
            .into())
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TaskStatus::Draft => "draft",
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Canceled => "canceled",
        };
        write!(f, "{s}")
    }
}

impl FromStr for TaskStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(TaskStatus::Draft),
            "todo" => Ok(TaskStatus::Todo),
            "in_progress" => Ok(TaskStatus::InProgress),
            "completed" => Ok(TaskStatus::Completed),
            "canceled" => Ok(TaskStatus::Canceled),
            _ => Err(DomainError::InvalidTaskStatus {
                value: s.to_string(),
            }
            .into()),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
pub enum Priority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
}

impl TryFrom<i32> for Priority {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Priority::P0),
            1 => Ok(Priority::P1),
            2 => Ok(Priority::P2),
            3 => Ok(Priority::P3),
            _ => Err(DomainError::InvalidPriority {
                value: value.to_string(),
            }
            .into()),
        }
    }
}

impl From<Priority> for i32 {
    fn from(p: Priority) -> i32 {
        p as i32
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Priority::P0 => "P0",
            Priority::P1 => "P1",
            Priority::P2 => "P2",
            Priority::P3 => "P3",
        };
        write!(f, "{s}")
    }
}

impl FromStr for Priority {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "p0" => Ok(Priority::P0),
            "p1" => Ok(Priority::P1),
            "p2" => Ok(Priority::P2),
            "p3" => Ok(Priority::P3),
            _ => Err(DomainError::InvalidPriority {
                value: s.to_string(),
            }
            .into()),
        }
    }
}

/// Sort direction for list endpoints.
///
/// Default `Asc` keeps the historical behavior (`ORDER BY id ASC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListOrder {
    #[default]
    Asc,
    Desc,
}

impl FromStr for ListOrder {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(ListOrder::Asc),
            "desc" => Ok(ListOrder::Desc),
            other => Err(DomainError::InvalidQueryParam {
                field: "order",
                value: other.to_string(),
            }),
        }
    }
}

/// Sort key for `list_tasks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskOrderBy {
    #[default]
    Id,
    UpdatedAt,
    Priority,
}

impl FromStr for TaskOrderBy {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "id" => Ok(TaskOrderBy::Id),
            "updated_at" => Ok(TaskOrderBy::UpdatedAt),
            "priority" => Ok(TaskOrderBy::Priority),
            other => Err(DomainError::InvalidQueryParam {
                field: "order_by",
                value: other.to_string(),
            }),
        }
    }
}

impl TaskOrderBy {
    /// Short label used in `CursorMismatch` errors.
    pub fn cursor_kind(&self) -> &'static str {
        match self {
            TaskOrderBy::Id => "id",
            TaskOrderBy::UpdatedAt => "updated_at",
            TaskOrderBy::Priority => "priority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DodItem {
    content: String,
    checked: bool,
}

impl DodItem {
    pub fn new(content: String, checked: bool) -> Self {
        Self { content, checked }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn checked(&self) -> bool {
        self.checked
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    id: TaskId,
    project_id: ProjectId,
    title: String,
    background: Option<String>,
    description: Option<String>,
    plan: Option<String>,
    priority: Priority,
    status: TaskStatus,
    assignee_session_id: Option<String>,
    assignee_user_id: Option<UserId>,
    created_at: String,
    updated_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    canceled_at: Option<String>,
    cancel_reason: Option<String>,
    branch: Option<String>,
    pr_url: Option<String>,
    contract_id: Option<ContractId>,
    metadata: Option<serde_json::Value>,
    definition_of_done: Vec<DodItem>,
    in_scope: Vec<String>,
    out_of_scope: Vec<String>,
    tags: Vec<String>,
    dependencies: Vec<TaskId>,
}

impl Task {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TaskId,
        project_id: ProjectId,
        title: String,
        background: Option<String>,
        description: Option<String>,
        plan: Option<String>,
        priority: Priority,
        status: TaskStatus,
        assignee_session_id: Option<String>,
        assignee_user_id: Option<UserId>,
        created_at: String,
        updated_at: String,
        started_at: Option<String>,
        completed_at: Option<String>,
        canceled_at: Option<String>,
        cancel_reason: Option<String>,
        branch: Option<String>,
        pr_url: Option<String>,
        contract_id: Option<ContractId>,
        metadata: Option<serde_json::Value>,
        definition_of_done: Vec<DodItem>,
        in_scope: Vec<String>,
        out_of_scope: Vec<String>,
        tags: Vec<String>,
        dependencies: Vec<TaskId>,
    ) -> Self {
        Self {
            id,
            project_id,
            title,
            background,
            description,
            plan,
            priority,
            status,
            assignee_session_id,
            assignee_user_id,
            created_at,
            updated_at,
            started_at,
            completed_at,
            canceled_at,
            cancel_reason,
            branch,
            pr_url,
            contract_id,
            metadata,
            definition_of_done,
            in_scope,
            out_of_scope,
            tags,
            dependencies,
        }
    }

    // --- Getters ---

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn background(&self) -> Option<&str> {
        self.background.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn plan(&self) -> Option<&str> {
        self.plan.as_deref()
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn status(&self) -> TaskStatus {
        self.status
    }

    pub fn assignee_session_id(&self) -> Option<&str> {
        self.assignee_session_id.as_deref()
    }

    pub fn assignee_user_id(&self) -> Option<UserId> {
        self.assignee_user_id
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    pub fn started_at(&self) -> Option<&str> {
        self.started_at.as_deref()
    }

    pub fn completed_at(&self) -> Option<&str> {
        self.completed_at.as_deref()
    }

    pub fn canceled_at(&self) -> Option<&str> {
        self.canceled_at.as_deref()
    }

    pub fn cancel_reason(&self) -> Option<&str> {
        self.cancel_reason.as_deref()
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn pr_url(&self) -> Option<&str> {
        self.pr_url.as_deref()
    }

    pub fn contract_id(&self) -> Option<ContractId> {
        self.contract_id
    }

    pub fn metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_ref()
    }

    pub fn definition_of_done(&self) -> &[DodItem] {
        &self.definition_of_done
    }

    pub fn in_scope(&self) -> &[String] {
        &self.in_scope
    }

    pub fn out_of_scope(&self) -> &[String] {
        &self.out_of_scope
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn dependencies(&self) -> &[TaskId] {
        &self.dependencies
    }

    // --- Query methods ---

    /// Returns true if this task is eligible for execution.
    ///
    /// A task is ready when:
    /// - status == Todo
    /// - All dependencies that exist in `dep_statuses` have status == Completed
    /// - Missing dependencies (not present in `dep_statuses`) are treated as non-blocking
    pub fn is_ready(&self, dep_statuses: &HashMap<TaskId, TaskStatus>) -> bool {
        self.status == TaskStatus::Todo
            && self.dependencies.iter().all(|dep_id| {
                dep_statuses
                    .get(dep_id)
                    .is_none_or(|s| *s == TaskStatus::Completed)
            })
    }

    // --- Update methods ---

    pub fn apply_update(
        mut self,
        params: &UpdateTaskParams,
        now: String,
    ) -> (Task, Vec<TaskEvent>) {
        let mut changed_fields: Vec<String> = Vec::new();
        if let Some(ref title) = params.title
            && self.title != *title
        {
            self.title = title.clone();
            changed_fields.push("title".to_string());
        }
        if let Some(ref background) = params.background
            && self.background != *background
        {
            self.background = background.clone();
            changed_fields.push("background".to_string());
        }
        if let Some(ref description) = params.description
            && self.description != *description
        {
            self.description = description.clone();
            changed_fields.push("description".to_string());
        }
        if let Some(ref plan) = params.plan
            && self.plan != *plan
        {
            self.plan = plan.clone();
            changed_fields.push("plan".to_string());
        }
        if let Some(priority) = params.priority
            && self.priority != priority
        {
            self.priority = priority;
            changed_fields.push("priority".to_string());
        }
        if let Some(ref assignee_session_id) = params.assignee_session_id
            && self.assignee_session_id != *assignee_session_id
        {
            self.assignee_session_id = assignee_session_id.clone();
            changed_fields.push("assignee_session_id".to_string());
        }
        if let Some(ref assignee_user_id) = params.assignee_user_id {
            let resolved: Option<UserId> = assignee_user_id.as_ref().map(|a| {
                a.as_id()
                    .expect("AssigneeUserId::SelfUser must be resolved before apply_update")
            });
            if self.assignee_user_id != resolved {
                self.assignee_user_id = resolved;
                changed_fields.push("assignee_user_id".to_string());
            }
        }
        if let Some(ref started_at) = params.started_at
            && self.started_at != *started_at
        {
            self.started_at = started_at.clone();
            changed_fields.push("started_at".to_string());
        }
        if let Some(ref completed_at) = params.completed_at
            && self.completed_at != *completed_at
        {
            self.completed_at = completed_at.clone();
            changed_fields.push("completed_at".to_string());
        }
        if let Some(ref canceled_at) = params.canceled_at
            && self.canceled_at != *canceled_at
        {
            self.canceled_at = canceled_at.clone();
            changed_fields.push("canceled_at".to_string());
        }
        if let Some(ref cancel_reason) = params.cancel_reason
            && self.cancel_reason != *cancel_reason
        {
            self.cancel_reason = cancel_reason.clone();
            changed_fields.push("cancel_reason".to_string());
        }
        if let Some(ref branch) = params.branch
            && self.branch != *branch
        {
            self.branch = branch.clone();
            changed_fields.push("branch".to_string());
        }
        if let Some(ref pr_url) = params.pr_url
            && self.pr_url != *pr_url
        {
            self.pr_url = pr_url.clone();
            changed_fields.push("pr_url".to_string());
        }
        if let Some(ref contract_id) = params.contract_id
            && self.contract_id != *contract_id
        {
            self.contract_id = *contract_id;
            changed_fields.push("contract_id".to_string());
        }
        if let Some(ref meta_update) = params.metadata {
            let new_metadata: Option<serde_json::Value> = match meta_update {
                MetadataUpdate::Clear => None,
                MetadataUpdate::Merge(patch) => {
                    shallow_merge_metadata(self.metadata.as_ref(), patch)
                }
                MetadataUpdate::Replace(value) => Some(value.clone()),
            };
            if self.metadata != new_metadata {
                self.metadata = new_metadata;
                changed_fields.push("metadata".to_string());
            }
        }
        if changed_fields.is_empty() {
            (self, vec![])
        } else {
            self.updated_at = now;
            (self, vec![TaskEvent::Updated { changed_fields }])
        }
    }

    pub fn apply_array_update(
        mut self,
        params: &UpdateTaskArrayParams,
        now: String,
    ) -> (Task, Vec<TaskEvent>) {
        let mut changed_fields: Vec<String> = Vec::new();

        // Tags
        let prev_tags = self.tags.clone();
        if let Some(ref set_tags) = params.set_tags {
            self.tags = set_tags.clone();
        }
        for tag in &params.add_tags {
            if !self.tags.contains(tag) {
                self.tags.push(tag.clone());
            }
        }
        if !params.remove_tags.is_empty() {
            self.tags.retain(|t| !params.remove_tags.contains(t));
        }
        if self.tags != prev_tags {
            changed_fields.push("tags".to_string());
        }

        // Definition of Done
        let prev_dod = self.definition_of_done.clone();
        if let Some(ref set_dod) = params.set_definition_of_done {
            self.definition_of_done = set_dod
                .iter()
                .map(|c| DodItem::new(c.clone(), false))
                .collect();
        }
        for content in &params.add_definition_of_done {
            self.definition_of_done
                .push(DodItem::new(content.clone(), false));
        }
        if !params.remove_definition_of_done.is_empty() {
            self.definition_of_done
                .retain(|d| !params.remove_definition_of_done.contains(&d.content));
        }
        if self.definition_of_done != prev_dod {
            changed_fields.push("definition_of_done".to_string());
        }

        // In scope
        let prev_in_scope = self.in_scope.clone();
        if let Some(ref set_in_scope) = params.set_in_scope {
            self.in_scope = set_in_scope.clone();
        }
        for item in &params.add_in_scope {
            if !self.in_scope.contains(item) {
                self.in_scope.push(item.clone());
            }
        }
        if !params.remove_in_scope.is_empty() {
            self.in_scope
                .retain(|i| !params.remove_in_scope.contains(i));
        }
        if self.in_scope != prev_in_scope {
            changed_fields.push("in_scope".to_string());
        }

        // Out of scope
        let prev_out_of_scope = self.out_of_scope.clone();
        if let Some(ref set_out_of_scope) = params.set_out_of_scope {
            self.out_of_scope = set_out_of_scope.clone();
        }
        for item in &params.add_out_of_scope {
            if !self.out_of_scope.contains(item) {
                self.out_of_scope.push(item.clone());
            }
        }
        if !params.remove_out_of_scope.is_empty() {
            self.out_of_scope
                .retain(|i| !params.remove_out_of_scope.contains(i));
        }
        if self.out_of_scope != prev_out_of_scope {
            changed_fields.push("out_of_scope".to_string());
        }

        if changed_fields.is_empty() {
            (self, vec![])
        } else {
            self.updated_at = now;
            (self, vec![TaskEvent::Updated { changed_fields }])
        }
    }

    // --- Aggregate methods ---

    /// Transition: Draft -> Todo.
    pub fn publish(mut self, now: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::Todo)?;
        self.updated_at = now;
        Ok((self, vec![TaskEvent::Published]))
    }

    /// Transition: Todo -> InProgress.
    pub fn start(
        mut self,
        assignee_session_id: Option<String>,
        assignee_user_id: Option<UserId>,
        started_at: String,
        metadata: Option<MetadataUpdate>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::InProgress)?;
        self.assignee_session_id = assignee_session_id;
        if let Some(starting_uid) = assignee_user_id {
            if let Some(current_uid) = self.assignee_user_id {
                if current_uid != starting_uid {
                    anyhow::bail!(
                        "task is assigned to user {current_uid}, cannot be started by user {starting_uid}"
                    );
                }
            } else {
                self.assignee_user_id = Some(starting_uid);
            }
        }
        if let Some(meta_update) = metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    self.metadata = None;
                }
                MetadataUpdate::Merge(patch) => {
                    self.metadata = shallow_merge_metadata(self.metadata.as_ref(), &patch);
                }
                MetadataUpdate::Replace(value) => {
                    self.metadata = Some(value);
                }
            }
        }
        self.updated_at = started_at.clone();
        self.started_at = Some(started_at);
        Ok((self, vec![TaskEvent::Started]))
    }

    /// Resume: refresh `assignee_session_id` and `metadata` on an already
    /// in-progress task. Not a status transition — `status` and `started_at`
    /// stay untouched. Rejects any non-`InProgress` status.
    pub fn resume(
        mut self,
        assignee_session_id: Option<String>,
        now: String,
        metadata: Option<MetadataUpdate>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if self.status != TaskStatus::InProgress {
            return Err(DomainError::InvalidStatusTransition {
                from: self.status.to_string(),
                to: TaskStatus::InProgress.to_string(),
            }
            .into());
        }
        self.assignee_session_id = assignee_session_id;
        if let Some(meta_update) = metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    self.metadata = None;
                }
                MetadataUpdate::Merge(patch) => {
                    self.metadata = shallow_merge_metadata(self.metadata.as_ref(), &patch);
                }
                MetadataUpdate::Replace(value) => {
                    self.metadata = Some(value);
                }
            }
        }
        self.updated_at = now;
        Ok((self, vec![TaskEvent::Resumed]))
    }

    /// Transition: InProgress -> Completed.
    ///
    /// Validates that all DoD items are checked before allowing completion.
    pub fn complete(mut self, completed_at: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        let unchecked_count = self
            .definition_of_done
            .iter()
            .filter(|d| !d.checked)
            .count();
        if unchecked_count > 0 {
            return Err(DomainError::CannotCompleteTask {
                task_id: self.id,
                reason: format!("{} unchecked DoD item(s)", unchecked_count),
            }
            .into());
        }
        self.status = self.status.transition_to(TaskStatus::Completed)?;
        self.updated_at = completed_at.clone();
        self.completed_at = Some(completed_at);
        Ok((self, vec![TaskEvent::Completed]))
    }

    /// Transition: active -> Canceled.
    pub fn cancel(
        mut self,
        canceled_at: String,
        reason: Option<String>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::Canceled)?;
        self.updated_at = canceled_at.clone();
        self.canceled_at = Some(canceled_at);
        self.cancel_reason = reason;
        Ok((self, vec![TaskEvent::Canceled]))
    }

    /// Add a dependency, validating self-dependency. Idempotent (no event if already present).
    pub fn add_dependency(
        mut self,
        dep_id: TaskId,
        now: Option<String>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if self.id == dep_id {
            return Err(DomainError::SelfDependency.into());
        }
        if !self.dependencies.contains(&dep_id) {
            self.dependencies.push(dep_id);
            if let Some(now) = now {
                self.updated_at = now;
            }
            Ok((self, vec![TaskEvent::DependencyAdded { dep_id }]))
        } else {
            Ok((self, vec![]))
        }
    }

    /// Remove a dependency, validating existence.
    pub fn remove_dependency(
        mut self,
        dep_id: TaskId,
        now: Option<String>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        let before = self.dependencies.len();
        self.dependencies.retain(|&d| d != dep_id);
        if self.dependencies.len() == before {
            return Err(DomainError::DependencyNotFound {
                task_id: self.id,
                dep_id,
            }
            .into());
        }
        if let Some(now) = now {
            self.updated_at = now;
        }
        Ok((self, vec![TaskEvent::DependencyRemoved { dep_id }]))
    }

    /// Replace all dependencies, validating no self-dependency.
    pub fn set_dependencies(
        mut self,
        dep_ids: &[TaskId],
        now: Option<String>,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        for &dep_id in dep_ids {
            if dep_id == self.id {
                return Err(DomainError::SelfDependency.into());
            }
        }
        self.dependencies = dep_ids.to_vec();
        if let Some(now) = now {
            self.updated_at = now;
        }
        Ok((
            self,
            vec![TaskEvent::DependenciesSet {
                dep_ids: dep_ids.to_vec(),
            }],
        ))
    }

    /// Check a DoD item by 1-based index.
    pub fn check_dod(
        mut self,
        index: usize,
        now: String,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id.into(),
                count: self.definition_of_done.len(),
            }
            .into());
        }
        self.definition_of_done[index - 1].checked = true;
        self.updated_at = now;
        Ok((self, vec![TaskEvent::DodChecked { index }]))
    }

    /// Uncheck a DoD item by 1-based index.
    pub fn uncheck_dod(
        mut self,
        index: usize,
        now: String,
    ) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id.into(),
                count: self.definition_of_done.len(),
            }
            .into());
        }
        self.definition_of_done[index - 1].checked = false;
        self.updated_at = now;
        Ok((self, vec![TaskEvent::DodUnchecked { index }]))
    }
}

/// Filter tasks to only those that are ready (eligible for execution).
///
/// This is the canonical definition of "ready" used across all backends.
/// SQL backends may implement equivalent logic in SQL for performance;
/// see [`select_next`] for the full selection specification.
pub fn filter_ready(tasks: Vec<Task>, dep_statuses: &HashMap<TaskId, TaskStatus>) -> Vec<Task> {
    tasks
        .into_iter()
        .filter(|t| t.is_ready(dep_statuses))
        .collect()
}

/// Select the next task to execute from a set of tasks.
///
/// Selection rules (canonical — all backends must produce equivalent results):
/// 1. Filter to ready tasks (status == Todo, all existing deps completed)
/// 2. Sort by: priority ASC (P0 first), created_at ASC, id ASC
/// 3. Return the first one
///
/// SQL backends may implement this as an optimized query; equivalence
/// is verified by integration tests.
pub fn select_next(tasks: Vec<Task>, dep_statuses: &HashMap<TaskId, TaskStatus>) -> Option<Task> {
    let mut ready = filter_ready(tasks, dep_statuses);
    ready.sort_by(|a, b| {
        a.priority()
            .cmp(&b.priority())
            .then_with(|| a.created_at().cmp(b.created_at()))
            .then_with(|| a.id().cmp(&b.id()))
    });
    ready.into_iter().next()
}

/// Assignee user ID that can be either a resolved numeric ID or the "self" sentinel.
///
/// When serialized to JSON, `SelfUser` becomes `"self"` and `Id(n)` becomes `n`.
/// The API layer resolves `SelfUser` to the authenticated user's ID.
/// In local (non-remote) mode, the CLI resolves `SelfUser` before it reaches the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssigneeUserId {
    /// The current authenticated user (resolved by the API or CLI).
    SelfUser,
    /// A specific numeric user ID.
    Id(UserId),
}

impl AssigneeUserId {
    /// Returns the numeric ID, or `None` if this is `SelfUser`.
    pub fn as_id(&self) -> Option<UserId> {
        match self {
            AssigneeUserId::Id(id) => Some(*id),
            AssigneeUserId::SelfUser => None,
        }
    }
}

impl Serialize for AssigneeUserId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AssigneeUserId::SelfUser => serializer.serialize_str("self"),
            AssigneeUserId::Id(id) => serializer.serialize_i64(id.0),
        }
    }
}

impl<'de> Deserialize<'de> for AssigneeUserId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = AssigneeUserId;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("\"self\" or an integer user ID")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                if v == "self" {
                    Ok(AssigneeUserId::SelfUser)
                } else {
                    Err(E::custom(format!(
                        "expected \"self\" or integer, got \"{v}\""
                    )))
                }
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(AssigneeUserId::Id(UserId(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(AssigneeUserId::Id(UserId(v as i64)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateTaskParams {
    pub title: String,
    pub background: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    #[serde(default)]
    pub definition_of_done: Vec<String>,
    #[serde(default)]
    pub in_scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<TaskId>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub assignee_user_id: Option<AssigneeUserId>,
    #[serde(default)]
    pub contract_id: Option<ContractId>,
}

/// Describes how to update metadata on a task.
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataUpdate {
    /// Remove all metadata (set to null).
    Clear,
    /// Shallow-merge the given JSON object into existing metadata.
    Merge(serde_json::Value),
    /// Replace existing metadata entirely with the given value.
    Replace(serde_json::Value),
}

/// Shallow-merge a JSON patch into existing metadata.
///
/// Rules (top-level keys only):
/// - Key in patch with non-null value → add or overwrite
/// - Key in patch with null value → delete from existing
/// - Key not in patch → preserve from existing
/// - If existing or patch is not an object → patch replaces entirely
/// - If existing is None → patch becomes the new metadata (null-valued keys removed)
pub fn shallow_merge_metadata(
    existing: Option<&serde_json::Value>,
    patch: &serde_json::Value,
) -> Option<serde_json::Value> {
    use serde_json::Value;

    let patch_obj = match patch {
        Value::Object(m) => m,
        // Non-object patch replaces entirely
        other => return Some(other.clone()),
    };

    let mut base = match existing {
        Some(Value::Object(m)) => m.clone(),
        Some(_) | None => serde_json::Map::new(),
    };

    for (k, v) in patch_obj {
        if v.is_null() {
            base.remove(k);
        } else {
            base.insert(k.clone(), v.clone());
        }
    }

    if base.is_empty() {
        None
    } else {
        Some(Value::Object(base))
    }
}

impl CreateTaskParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        validate_string_length("title", &self.title, MAX_TITLE_LEN)?;
        validate_optional_string_length("background", &self.background, MAX_LONG_TEXT_LEN)?;
        validate_optional_string_length("description", &self.description, MAX_LONG_TEXT_LEN)?;
        validate_string_vec_items("tags", &self.tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        validate_string_vec_items(
            "definition_of_done",
            &self.definition_of_done,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        validate_string_vec_items(
            "in_scope",
            &self.in_scope,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        validate_string_vec_items(
            "out_of_scope",
            &self.out_of_scope,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        validate_optional_string_length("branch", &self.branch, MAX_SHORT_TEXT_LEN)?;
        validate_optional_string_length("pr_url", &self.pr_url, MAX_SHORT_TEXT_LEN)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct UpdateTaskParams {
    pub title: Option<String>,
    pub background: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub plan: Option<Option<String>>,
    pub priority: Option<Priority>,
    pub assignee_session_id: Option<Option<String>>,
    pub assignee_user_id: Option<Option<AssigneeUserId>>,
    pub started_at: Option<Option<String>>,
    pub completed_at: Option<Option<String>>,
    pub canceled_at: Option<Option<String>>,
    pub cancel_reason: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub pr_url: Option<Option<String>>,
    pub contract_id: Option<Option<ContractId>>,
    pub metadata: Option<MetadataUpdate>,
}

impl UpdateTaskParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        if let Some(ref title) = self.title {
            validate_string_length("title", title, MAX_TITLE_LEN)?;
        }
        validate_optional_nullable_string_length(
            "background",
            &self.background,
            MAX_LONG_TEXT_LEN,
        )?;
        validate_optional_nullable_string_length(
            "description",
            &self.description,
            MAX_LONG_TEXT_LEN,
        )?;
        validate_optional_nullable_string_length("plan", &self.plan, MAX_LONG_TEXT_LEN)?;
        validate_optional_nullable_string_length(
            "cancel_reason",
            &self.cancel_reason,
            MAX_LONG_TEXT_LEN,
        )?;
        validate_optional_nullable_string_length("branch", &self.branch, MAX_SHORT_TEXT_LEN)?;
        validate_optional_nullable_string_length("pr_url", &self.pr_url, MAX_SHORT_TEXT_LEN)?;
        if let Some(Some(ref session_id)) = self.assignee_session_id {
            validate_string_length("assignee_session_id", session_id, MAX_SESSION_ID_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct ListTasksFilter {
    pub statuses: Vec<TaskStatus>,
    pub tags: Vec<String>,
    pub depends_on: Option<TaskId>,
    pub ready: bool,
    pub assignee_user_id: Option<UserId>,
    // Unresolved "self" intent, used to carry `?assignee_user_id=self` through a
    // relay that has no auth context to resolve it; the upstream (which does)
    // converts it to `assignee_user_id`.
    pub assignee_self: bool,
    pub include_unassigned: bool,
    pub metadata: HashMap<String, serde_json::Value>,
    pub contract_id: Option<ContractId>,
    pub id_min: Option<TaskId>,
    pub id_max: Option<TaskId>,
    pub limit: Option<u32>,
    /// OR-combined priority filter. Empty means no priority filter.
    pub priorities: Vec<Priority>,
    /// Case-insensitive substring filter on `title`. Already trimmed and
    /// length-validated by the handler; repositories treat `Some("")` as no
    /// filter defensively but the canonical "no filter" representation is `None`.
    pub title: Option<String>,
    /// Composite cursor pinned to the same `order_by` axis as this filter.
    /// `Id` for `order_by=Id`, `Tagged::UpdatedAt` for `order_by=UpdatedAt`, etc.
    /// Mismatch is a programmer error caught by the handler before reaching the
    /// repository.
    pub after: Option<crate::domain::pagination::CursorPayload>,
    pub order_by: TaskOrderBy,
    pub order: ListOrder,
}

pub type ListTasksPage = crate::domain::pagination::ListPage<Task>;

/// Filter / paging inputs for `list_dependencies` (task deps).
///
/// Cursor payload is the `TaskId` of the last returned dependency, so sorting
/// must be by `depends_on_task_id ASC` to keep the cursor stable.
#[derive(Clone, Default)]
pub struct ListTaskDepsFilter {
    pub limit: Option<u32>,
    pub after: Option<TaskId>,
}

#[derive(Clone)]
pub struct UpdateTaskArrayParams {
    pub set_tags: Option<Vec<String>>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub set_definition_of_done: Option<Vec<String>>,
    pub add_definition_of_done: Vec<String>,
    pub remove_definition_of_done: Vec<String>,
    pub set_in_scope: Option<Vec<String>>,
    pub add_in_scope: Vec<String>,
    pub remove_in_scope: Vec<String>,
    pub set_out_of_scope: Option<Vec<String>>,
    pub add_out_of_scope: Vec<String>,
    pub remove_out_of_scope: Vec<String>,
}

impl UpdateTaskArrayParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        if let Some(ref tags) = self.set_tags {
            validate_string_vec_items("set_tags", tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        }
        validate_string_vec_items("add_tags", &self.add_tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        if let Some(ref dod) = self.set_definition_of_done {
            validate_string_vec_items(
                "set_definition_of_done",
                dod,
                MAX_SHORT_TEXT_LEN,
                MAX_ITEMS_COUNT,
            )?;
        }
        validate_string_vec_items(
            "add_definition_of_done",
            &self.add_definition_of_done,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        if let Some(ref scope) = self.set_in_scope {
            validate_string_vec_items("set_in_scope", scope, MAX_SHORT_TEXT_LEN, MAX_ITEMS_COUNT)?;
        }
        validate_string_vec_items(
            "add_in_scope",
            &self.add_in_scope,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        if let Some(ref scope) = self.set_out_of_scope {
            validate_string_vec_items(
                "set_out_of_scope",
                scope,
                MAX_SHORT_TEXT_LEN,
                MAX_ITEMS_COUNT,
            )?;
        }
        validate_string_vec_items(
            "add_out_of_scope",
            &self.add_out_of_scope,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        Ok(())
    }
}

// --- Domain functions ---

/// Expand `${task_id}` placeholders in a branch template.
pub fn expand_branch_template(template: &str, task_id: TaskId) -> String {
    template.replace("${task_id}", &task_id.to_string())
}

/// Compute newly unblocked tasks by comparing current ready tasks against
/// the set of ready task IDs captured before a completion.
///
/// This is a pure function — the caller fetches ready tasks from the backend.
pub fn compute_unblocked(
    current_ready: &[Task],
    prev_ready_ids: &HashSet<TaskId>,
) -> Vec<UnblockedTask> {
    current_ready
        .iter()
        .filter(|t| !prev_ready_ids.contains(&t.id()))
        .map(|t| {
            UnblockedTask::new(
                t.id(),
                t.title().to_string(),
                t.priority(),
                t.metadata().cloned(),
            )
        })
        .collect()
}

/// Domain policy for task completion workflow.
pub struct CompletionPolicy {
    merge_via: MergeVia,
}

impl CompletionPolicy {
    pub fn new(merge_via: MergeVia) -> Self {
        Self { merge_via }
    }

    /// Returns the PR URL that must be verified, or `None` if no PR check is needed.
    ///
    /// Returns `Err` if the merge_via mode requires a PR URL but none is set on the task.
    pub fn required_pr_url<'a>(
        &self,
        task: &'a Task,
        skip_pr_check: bool,
    ) -> Result<Option<&'a str>> {
        if skip_pr_check || self.merge_via != MergeVia::Pr {
            return Ok(None);
        }
        let pr_url = task.pr_url().ok_or_else(|| {
            anyhow::anyhow!(
                "cannot complete task #{}: merge_via is pr but no pr_url is set. \
                 Use `senko task edit {} --pr-url <url>` to set it.",
                task.id(),
                task.id(),
            )
        })?;
        Ok(Some(pr_url))
    }
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task>;
    async fn update_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskParams,
    ) -> Result<Task>;
    async fn update_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()>;
    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()>;
    async fn list_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        filter: &ListTaskDepsFilter,
    ) -> Result<super::pagination::ListPage<Task>>;
    async fn save(&self, task: &Task) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_display_roundtrip() {
        let statuses = [
            TaskStatus::Draft,
            TaskStatus::Todo,
            TaskStatus::InProgress,
            TaskStatus::Completed,
            TaskStatus::Canceled,
        ];
        for status in statuses {
            let s = status.to_string();
            let parsed: TaskStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn status_serde_roundtrip() {
        let status = TaskStatus::InProgress;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn status_invalid() {
        assert!("invalid".parse::<TaskStatus>().is_err());
    }

    #[test]
    fn priority_try_from_roundtrip() {
        for v in 0..=3 {
            let p = Priority::try_from(v).unwrap();
            let back: i32 = p.into();
            assert_eq!(back, v);
        }
    }

    #[test]
    fn priority_invalid() {
        assert!(Priority::try_from(4).is_err());
        assert!(Priority::try_from(-1).is_err());
    }

    #[test]
    fn priority_from_str() {
        assert_eq!("p0".parse::<Priority>().unwrap(), Priority::P0);
        assert_eq!("P1".parse::<Priority>().unwrap(), Priority::P1);
        assert_eq!("p2".parse::<Priority>().unwrap(), Priority::P2);
        assert_eq!("P3".parse::<Priority>().unwrap(), Priority::P3);
    }

    #[test]
    fn priority_from_str_invalid() {
        assert!("p4".parse::<Priority>().is_err());
        assert!("high".parse::<Priority>().is_err());
    }

    #[test]
    fn priority_serde_roundtrip() {
        let p = Priority::P2;
        let json = serde_json::to_string(&p).unwrap();
        let parsed: Priority = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn allowed_transitions() {
        use TaskStatus::*;
        let allowed = [
            (Draft, Todo),
            (Todo, InProgress),
            (InProgress, Completed),
            (Draft, Canceled),
            (Todo, Canceled),
            (InProgress, Canceled),
        ];
        for (from, to) in allowed {
            assert!(
                from.can_transition_to(to),
                "{from} -> {to} should be allowed"
            );
            assert!(
                from.transition_to(to).is_ok(),
                "{from} -> {to} should be ok"
            );
        }
    }

    #[test]
    fn forbidden_transitions() {
        use TaskStatus::*;
        let forbidden = [
            (Completed, Draft),
            (Completed, Todo),
            (Completed, InProgress),
            (Completed, Canceled),
            (Canceled, Draft),
            (Canceled, Todo),
            (Canceled, InProgress),
            (Canceled, Completed),
            (Draft, InProgress),
            (Draft, Completed),
            (Todo, Completed),
            (Todo, Draft),
            (InProgress, Todo),
            (InProgress, Draft),
        ];
        for (from, to) in forbidden {
            assert!(
                !from.can_transition_to(to),
                "{from} -> {to} should be forbidden"
            );
            assert!(
                from.transition_to(to).is_err(),
                "{from} -> {to} should be err"
            );
        }
    }

    #[test]
    fn self_transitions_forbidden() {
        use TaskStatus::*;
        for status in [Draft, Todo, InProgress, Completed, Canceled] {
            assert!(
                !status.can_transition_to(status),
                "{status} -> {status} should be forbidden"
            );
        }
    }

    // --- Task aggregate method tests ---

    fn make_task(status: TaskStatus) -> Task {
        Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            status,
            None,
            None,
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )
    }

    fn default_update_params() -> UpdateTaskParams {
        UpdateTaskParams {
            title: None,
            background: None,
            description: None,
            plan: None,
            priority: None,
            assignee_session_id: None,
            assignee_user_id: None,
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            contract_id: None,
            metadata: None,
        }
    }

    #[test]
    fn task_publish_from_draft() {
        let task = make_task(TaskStatus::Draft);
        let (task, events) = task.publish("2026-01-02T00:00:00Z".to_string()).unwrap();
        assert_eq!(events, vec![TaskEvent::Published]);
        assert_eq!(task.status(), TaskStatus::Todo);
        assert_eq!(task.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn task_publish_from_todo_fails() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.publish("2026-01-02T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_start_from_todo() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task
            .start(
                Some("session-1".to_string()),
                None,
                "2026-01-02T00:00:00Z".to_string(),
                None,
            )
            .unwrap();
        assert_eq!(events, vec![TaskEvent::Started]);
        assert_eq!(task.status(), TaskStatus::InProgress);
        assert_eq!(task.assignee_session_id(), Some("session-1"));
        assert_eq!(task.started_at(), Some("2026-01-02T00:00:00Z"));
        assert_eq!(task.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn task_start_with_metadata_replace() {
        let task = make_task(TaskStatus::Todo);
        let meta = serde_json::json!({"key": "value"});
        let (task, _) = task
            .start(
                None,
                None,
                "2026-01-02T00:00:00Z".to_string(),
                Some(MetadataUpdate::Replace(meta.clone())),
            )
            .unwrap();
        assert_eq!(task.metadata(), Some(&meta));
    }

    #[test]
    fn task_start_with_metadata_merge() {
        let mut task = make_task(TaskStatus::Todo);
        task.metadata = Some(serde_json::json!({"a": 1, "b": 2}));
        let (task, _) = task
            .start(
                None,
                None,
                "2026-01-02T00:00:00Z".to_string(),
                Some(MetadataUpdate::Merge(serde_json::json!({"b": 3, "c": 4}))),
            )
            .unwrap();
        assert_eq!(
            task.metadata(),
            Some(&serde_json::json!({"a": 1, "b": 3, "c": 4}))
        );
    }

    #[test]
    fn task_start_without_metadata_preserves_existing() {
        let mut task = make_task(TaskStatus::Todo);
        let existing = serde_json::json!({"existing": true});
        task.metadata = Some(existing.clone());
        let (task, _) = task
            .start(None, None, "2026-01-02T00:00:00Z".to_string(), None)
            .unwrap();
        assert_eq!(task.metadata(), Some(&existing));
    }

    #[test]
    fn task_start_from_draft_fails() {
        let task = make_task(TaskStatus::Draft);
        assert!(
            task.start(None, None, "2026-01-02T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    #[test]
    fn start_unassigned_task_sets_assignee() {
        let task = make_task(TaskStatus::Todo);
        assert_eq!(task.assignee_user_id(), None);
        let (task, _) = task
            .start(
                None,
                Some(UserId(5)),
                "2026-01-02T00:00:00Z".to_string(),
                None,
            )
            .unwrap();
        assert_eq!(task.assignee_user_id(), Some(UserId(5)));
    }

    // --- resume() tests ---

    fn make_in_progress_task() -> Task {
        let mut t = make_task(TaskStatus::InProgress);
        t.assignee_session_id = Some("session-1".to_string());
        t.started_at = Some("2026-01-02T00:00:00Z".to_string());
        t.updated_at = "2026-01-02T00:00:00Z".to_string();
        t
    }

    #[test]
    fn task_resume_from_in_progress_updates_session_and_metadata() {
        let mut t = make_in_progress_task();
        t.metadata = Some(serde_json::json!({"a": 1}));
        let (task, events) = t
            .resume(
                Some("session-2".to_string()),
                "2026-01-03T00:00:00Z".to_string(),
                Some(MetadataUpdate::Merge(serde_json::json!({"b": 2}))),
            )
            .unwrap();
        assert_eq!(events, vec![TaskEvent::Resumed]);
        assert_eq!(task.status(), TaskStatus::InProgress);
        assert_eq!(task.assignee_session_id(), Some("session-2"));
        assert_eq!(task.metadata(), Some(&serde_json::json!({"a": 1, "b": 2})));
        assert_eq!(task.updated_at(), "2026-01-03T00:00:00Z");
    }

    #[test]
    fn task_resume_does_not_modify_started_at() {
        let t = make_in_progress_task();
        let original_started_at = t.started_at().map(|s| s.to_string());
        let (task, _) = t
            .resume(None, "2026-01-03T12:34:56Z".to_string(), None)
            .unwrap();
        assert_eq!(
            task.started_at().map(|s| s.to_string()),
            original_started_at
        );
    }

    #[test]
    fn task_resume_clears_session_id_when_none() {
        let t = make_in_progress_task();
        let (task, _) = t
            .resume(None, "2026-01-03T00:00:00Z".to_string(), None)
            .unwrap();
        assert_eq!(task.assignee_session_id(), None);
    }

    #[test]
    fn task_resume_from_draft_fails() {
        let task = make_task(TaskStatus::Draft);
        assert!(
            task.resume(None, "2026-01-03T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    #[test]
    fn task_resume_from_todo_fails() {
        let task = make_task(TaskStatus::Todo);
        assert!(
            task.resume(None, "2026-01-03T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    #[test]
    fn task_resume_from_completed_fails() {
        let task = make_task(TaskStatus::Completed);
        assert!(
            task.resume(None, "2026-01-03T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    #[test]
    fn task_resume_from_canceled_fails() {
        let task = make_task(TaskStatus::Canceled);
        assert!(
            task.resume(None, "2026-01-03T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    // --- shallow_merge_metadata tests ---

    #[test]
    fn shallow_merge_adds_new_keys() {
        let existing = serde_json::json!({"a": 1});
        let patch = serde_json::json!({"b": 2});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1, "b": 2})));
    }

    #[test]
    fn shallow_merge_overwrites_existing_keys() {
        let existing = serde_json::json!({"a": 1, "b": 2});
        let patch = serde_json::json!({"b": 99});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1, "b": 99})));
    }

    #[test]
    fn shallow_merge_null_deletes_key() {
        let existing = serde_json::json!({"a": 1, "b": 2});
        let patch = serde_json::json!({"b": null});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn shallow_merge_null_deletes_all_returns_none() {
        let existing = serde_json::json!({"a": 1});
        let patch = serde_json::json!({"a": null});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, None);
    }

    #[test]
    fn shallow_merge_preserves_unmentioned_keys() {
        let existing = serde_json::json!({"a": 1, "b": 2, "c": 3});
        let patch = serde_json::json!({"b": 99});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1, "b": 99, "c": 3})));
    }

    #[test]
    fn shallow_merge_nested_objects_replaced_not_merged() {
        let existing = serde_json::json!({"nested": {"x": 1, "y": 2}});
        let patch = serde_json::json!({"nested": {"z": 3}});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"nested": {"z": 3}})));
    }

    #[test]
    fn shallow_merge_with_none_existing() {
        let patch = serde_json::json!({"a": 1, "b": null});
        let result = shallow_merge_metadata(None, &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn shallow_merge_non_object_patch_replaces() {
        let existing = serde_json::json!({"a": 1});
        let patch = serde_json::json!("string_value");
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!("string_value")));
    }

    #[test]
    fn shallow_merge_non_object_existing_uses_patch_as_base() {
        let existing = serde_json::json!("old_string");
        let patch = serde_json::json!({"a": 1});
        let result = shallow_merge_metadata(Some(&existing), &patch);
        assert_eq!(result, Some(serde_json::json!({"a": 1})));
    }

    // --- apply_update metadata tests ---

    #[test]
    fn apply_update_metadata_clear() {
        let mut task = make_task(TaskStatus::Todo);
        task.metadata = Some(serde_json::json!({"a": 1}));
        let params = UpdateTaskParams {
            metadata: Some(MetadataUpdate::Clear),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.metadata(), None);
        assert_eq!(
            events,
            vec![TaskEvent::Updated {
                changed_fields: vec!["metadata".to_string()]
            }]
        );
    }

    #[test]
    fn apply_update_metadata_replace() {
        let mut task = make_task(TaskStatus::Todo);
        task.metadata = Some(serde_json::json!({"a": 1}));
        let new_meta = serde_json::json!({"x": 99});
        let params = UpdateTaskParams {
            metadata: Some(MetadataUpdate::Replace(new_meta.clone())),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.metadata(), Some(&new_meta));
        assert_eq!(
            events,
            vec![TaskEvent::Updated {
                changed_fields: vec!["metadata".to_string()]
            }]
        );
    }

    #[test]
    fn apply_update_metadata_merge() {
        let mut task = make_task(TaskStatus::Todo);
        task.metadata = Some(serde_json::json!({"a": 1, "b": 2}));
        let params = UpdateTaskParams {
            metadata: Some(MetadataUpdate::Merge(serde_json::json!({"b": 3, "c": 4}))),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            task.metadata(),
            Some(&serde_json::json!({"a": 1, "b": 3, "c": 4}))
        );
        assert_eq!(
            events,
            vec![TaskEvent::Updated {
                changed_fields: vec!["metadata".to_string()]
            }]
        );
    }

    #[test]
    fn apply_update_emits_updated_with_single_field() {
        let task = make_task(TaskStatus::Todo);
        let params = UpdateTaskParams {
            title: Some("new-title".to_string()),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.title(), "new-title");
        assert_eq!(task.updated_at(), "2026-01-02T00:00:00Z");
        assert_eq!(
            events,
            vec![TaskEvent::Updated {
                changed_fields: vec!["title".to_string()]
            }]
        );
    }

    #[test]
    fn apply_update_emits_updated_with_multiple_fields() {
        let task = make_task(TaskStatus::Todo);
        let params = UpdateTaskParams {
            title: Some("new-title".to_string()),
            description: Some(Some("new-desc".to_string())),
            priority: Some(Priority::P0),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.title(), "new-title");
        assert_eq!(task.description(), Some("new-desc"));
        assert_eq!(task.priority(), Priority::P0);
        match &events[..] {
            [TaskEvent::Updated { changed_fields }] => {
                assert_eq!(changed_fields.len(), 3);
                assert!(changed_fields.contains(&"title".to_string()));
                assert!(changed_fields.contains(&"description".to_string()));
                assert!(changed_fields.contains(&"priority".to_string()));
            }
            other => panic!("expected single Updated event, got {:?}", other),
        }
    }

    #[test]
    fn apply_update_no_op_returns_empty_events() {
        // Case 1: all params None
        let task = make_task(TaskStatus::Todo);
        let original_updated_at = task.updated_at().to_string();
        let params = default_update_params();
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
        assert_eq!(task.updated_at(), original_updated_at);

        // Case 2: Some(現在値) — values unchanged
        let task = make_task(TaskStatus::Todo);
        let original_updated_at = task.updated_at().to_string();
        let params = UpdateTaskParams {
            title: Some(task.title().to_string()),
            priority: Some(task.priority()),
            description: Some(task.description().map(|s| s.to_string())),
            ..default_update_params()
        };
        let (task, events) = task.apply_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
        assert_eq!(task.updated_at(), original_updated_at);
    }

    fn default_update_array_params() -> UpdateTaskArrayParams {
        UpdateTaskArrayParams {
            set_tags: None,
            add_tags: vec![],
            remove_tags: vec![],
            set_definition_of_done: None,
            add_definition_of_done: vec![],
            remove_definition_of_done: vec![],
            set_in_scope: None,
            add_in_scope: vec![],
            remove_in_scope: vec![],
            set_out_of_scope: None,
            add_out_of_scope: vec![],
            remove_out_of_scope: vec![],
        }
    }

    #[test]
    fn apply_array_update_emits_updated_with_single_field() {
        let task = make_task(TaskStatus::Todo);
        let params = UpdateTaskArrayParams {
            add_tags: vec!["backend".to_string()],
            ..default_update_array_params()
        };
        let (task, events) = task.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.tags(), &["backend".to_string()]);
        assert_eq!(
            events,
            vec![TaskEvent::Updated {
                changed_fields: vec!["tags".to_string()]
            }]
        );
    }

    #[test]
    fn apply_array_update_emits_updated_with_multiple_fields() {
        let task = make_task(TaskStatus::Todo);
        let params = UpdateTaskArrayParams {
            add_tags: vec!["backend".to_string()],
            add_definition_of_done: vec!["unit test".to_string()],
            set_in_scope: Some(vec!["domain".to_string()]),
            ..default_update_array_params()
        };
        let (task, events) = task.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(task.tags(), &["backend".to_string()]);
        assert_eq!(task.definition_of_done().len(), 1);
        assert_eq!(task.in_scope(), &["domain".to_string()]);
        match &events[..] {
            [TaskEvent::Updated { changed_fields }] => {
                assert_eq!(changed_fields.len(), 3);
                assert!(changed_fields.contains(&"tags".to_string()));
                assert!(changed_fields.contains(&"definition_of_done".to_string()));
                assert!(changed_fields.contains(&"in_scope".to_string()));
            }
            other => panic!("expected single Updated event, got {:?}", other),
        }
    }

    #[test]
    fn apply_array_update_no_op_returns_empty_events() {
        // Case 1: all params empty / None
        let task = make_task(TaskStatus::Todo);
        let original_updated_at = task.updated_at().to_string();
        let params = default_update_array_params();
        let (task, events) = task.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
        assert_eq!(task.updated_at(), original_updated_at);

        // Case 2: set_tags / add_tags resulting in identical state
        let mut task = make_task(TaskStatus::Todo);
        task.tags = vec!["a".to_string(), "b".to_string()];
        let original_updated_at = task.updated_at().to_string();
        let params = UpdateTaskArrayParams {
            set_tags: Some(vec!["a".to_string(), "b".to_string()]),
            add_tags: vec!["a".to_string()],
            ..default_update_array_params()
        };
        let (task, events) = task.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
        assert_eq!(task.updated_at(), original_updated_at);
        assert_eq!(task.tags(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn start_self_assigned_task_succeeds() {
        let task = Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::Todo,
            None,
            Some(UserId(5)),
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let (task, _) = task
            .start(
                None,
                Some(UserId(5)),
                "2026-01-02T00:00:00Z".to_string(),
                None,
            )
            .unwrap();
        assert_eq!(task.assignee_user_id(), Some(UserId(5)));
    }

    #[test]
    fn start_other_assigned_task_fails() {
        let task = Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::Todo,
            None,
            Some(UserId(5)),
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let err = task
            .start(
                None,
                Some(UserId(99)),
                "2026-01-02T00:00:00Z".to_string(),
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("assigned to user 5"));
        assert!(err.to_string().contains("cannot be started by user 99"));
    }

    #[test]
    fn start_with_none_user_preserves_assignee() {
        let task = Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::Todo,
            None,
            Some(UserId(5)),
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let (task, _) = task
            .start(None, None, "2026-01-02T00:00:00Z".to_string(), None)
            .unwrap();
        assert_eq!(task.assignee_user_id(), Some(UserId(5)));
    }

    #[test]
    fn start_with_none_user_unassigned_stays_none() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task
            .start(None, None, "2026-01-02T00:00:00Z".to_string(), None)
            .unwrap();
        assert_eq!(task.assignee_user_id(), None);
    }

    #[test]
    fn task_complete_from_in_progress() {
        let task = make_task(TaskStatus::InProgress);
        let (task, events) = task.complete("2026-01-03T00:00:00Z".to_string()).unwrap();
        assert_eq!(events, vec![TaskEvent::Completed]);
        assert_eq!(task.status(), TaskStatus::Completed);
        assert_eq!(task.completed_at(), Some("2026-01-03T00:00:00Z"));
        assert_eq!(task.updated_at(), "2026-01-03T00:00:00Z");
    }

    #[test]
    fn task_complete_from_todo_fails() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.complete("2026-01-03T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_complete_with_unchecked_dod_fails() {
        let task = make_task_with_dod();
        let err = task
            .complete("2026-01-03T00:00:00Z".to_string())
            .unwrap_err();
        assert!(err.to_string().contains("unchecked DoD item(s)"));
    }

    #[test]
    fn task_complete_with_all_dod_checked() {
        let task = make_task_with_dod();
        let (task, _) = task
            .check_dod(1, "2026-01-03T00:00:00Z".to_string())
            .unwrap();
        let (task, _) = task
            .check_dod(2, "2026-01-03T00:00:00Z".to_string())
            .unwrap();
        let (task, _) = task.complete("2026-01-03T00:00:00Z".to_string()).unwrap();
        assert_eq!(task.status(), TaskStatus::Completed);
    }

    #[test]
    fn task_cancel_from_draft() {
        let task = make_task(TaskStatus::Draft);
        let (task, events) = task
            .cancel(
                "2026-01-04T00:00:00Z".to_string(),
                Some("not needed".to_string()),
            )
            .unwrap();
        assert_eq!(events, vec![TaskEvent::Canceled]);
        assert_eq!(task.status(), TaskStatus::Canceled);
        assert_eq!(task.canceled_at(), Some("2026-01-04T00:00:00Z"));
        assert_eq!(task.cancel_reason(), Some("not needed"));
        assert_eq!(task.updated_at(), "2026-01-04T00:00:00Z");
    }

    #[test]
    fn task_cancel_from_in_progress() {
        let task = make_task(TaskStatus::InProgress);
        let (task, events) = task
            .cancel("2026-01-04T00:00:00Z".to_string(), None)
            .unwrap();
        assert_eq!(events, vec![TaskEvent::Canceled]);
        assert_eq!(task.status(), TaskStatus::Canceled);
        assert_eq!(task.updated_at(), "2026-01-04T00:00:00Z");
    }

    #[test]
    fn task_cancel_from_completed_fails() {
        let task = make_task(TaskStatus::Completed);
        assert!(
            task.cancel("2026-01-04T00:00:00Z".to_string(), None)
                .is_err()
        );
    }

    // --- Dependency management tests ---

    #[test]
    fn task_add_dependency() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task.add_dependency(TaskId(2), None).unwrap();
        assert_eq!(task.dependencies(), &[TaskId(2)]);
        assert_eq!(
            events,
            vec![TaskEvent::DependencyAdded { dep_id: TaskId(2) }]
        );
    }

    #[test]
    fn task_add_dependency_self_error() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.add_dependency(TaskId(1), None).is_err());
    }

    #[test]
    fn task_add_dependency_idempotent() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task.add_dependency(TaskId(2), None).unwrap();
        assert_eq!(events.len(), 1);
        let (task, events) = task.add_dependency(TaskId(2), None).unwrap();
        assert!(events.is_empty());
        assert_eq!(task.dependencies(), &[TaskId(2)]);
    }

    #[test]
    fn task_remove_dependency() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.add_dependency(TaskId(2), None).unwrap();
        let (task, _) = task.add_dependency(TaskId(3), None).unwrap();
        let (task, events) = task.remove_dependency(TaskId(2), None).unwrap();
        assert_eq!(task.dependencies(), &[TaskId(3)]);
        assert_eq!(
            events,
            vec![TaskEvent::DependencyRemoved { dep_id: TaskId(2) }]
        );
    }

    #[test]
    fn task_remove_dependency_not_found() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.remove_dependency(TaskId(99), None).is_err());
    }

    #[test]
    fn task_set_dependencies() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.add_dependency(TaskId(2), None).unwrap();
        let (task, events) = task
            .set_dependencies(&[TaskId(3), TaskId(4)], None)
            .unwrap();
        assert_eq!(task.dependencies(), &[TaskId(3), TaskId(4)]);
        assert_eq!(
            events,
            vec![TaskEvent::DependenciesSet {
                dep_ids: vec![TaskId(3), TaskId(4)]
            }]
        );
    }

    #[test]
    fn task_set_dependencies_self_error() {
        let task = make_task(TaskStatus::Todo);
        assert!(
            task.set_dependencies(&[TaskId(1), TaskId(2)], None)
                .is_err()
        );
    }

    // --- DoD operation tests ---

    fn make_task_with_dod() -> Task {
        Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::InProgress,
            None,
            None,
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![
                DodItem::new("Write tests".to_string(), false),
                DodItem::new("Update docs".to_string(), false),
            ],
            vec![],
            vec![],
            vec![],
            vec![],
        )
    }

    #[test]
    fn task_check_dod() {
        let task = make_task_with_dod();
        let (task, events) = task
            .check_dod(1, "2026-01-05T00:00:00Z".to_string())
            .unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());
        assert_eq!(task.updated_at(), "2026-01-05T00:00:00Z");
        assert_eq!(events, vec![TaskEvent::DodChecked { index: 1 }]);
    }

    #[test]
    fn task_uncheck_dod() {
        let task = Task::new(
            TaskId(1),
            ProjectId(1),
            "test".to_string(),
            None,
            None,
            None,
            Priority::P2,
            TaskStatus::InProgress,
            None,
            None,
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![
                DodItem::new("Write tests".to_string(), true),
                DodItem::new("Update docs".to_string(), false),
            ],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let (task, events) = task
            .uncheck_dod(1, "2026-01-05T00:00:00Z".to_string())
            .unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert_eq!(task.updated_at(), "2026-01-05T00:00:00Z");
        assert_eq!(events, vec![TaskEvent::DodUnchecked { index: 1 }]);
    }

    #[test]
    fn task_check_dod_index_zero() {
        let task = make_task_with_dod();
        assert!(
            task.check_dod(0, "2026-01-05T00:00:00Z".to_string())
                .is_err()
        );
    }

    #[test]
    fn task_check_dod_index_out_of_range() {
        let task = make_task_with_dod();
        assert!(
            task.check_dod(3, "2026-01-05T00:00:00Z".to_string())
                .is_err()
        );
    }

    #[test]
    fn task_check_dod_empty_list() {
        let task = make_task(TaskStatus::InProgress);
        assert!(
            task.check_dod(1, "2026-01-05T00:00:00Z".to_string())
                .is_err()
        );
    }

    // --- expand_branch_template tests ---

    #[test]
    fn expand_branch_template_replaces_task_id() {
        assert_eq!(
            super::expand_branch_template("feature/${task_id}-impl", TaskId(42)),
            "feature/42-impl"
        );
    }

    #[test]
    fn expand_branch_template_no_placeholder() {
        assert_eq!(
            super::expand_branch_template("feature/my-branch", TaskId(42)),
            "feature/my-branch"
        );
    }

    #[test]
    fn expand_branch_template_multiple_placeholders() {
        assert_eq!(
            super::expand_branch_template("${task_id}/${task_id}", TaskId(7)),
            "7/7"
        );
    }

    // --- compute_unblocked tests ---

    #[test]
    fn compute_unblocked_finds_newly_ready() {
        let prev_ready_ids: HashSet<TaskId> = [TaskId(1), TaskId(3)].into_iter().collect();
        let current_ready = vec![
            make_task_with_id(1, TaskStatus::Todo),
            make_task_with_id(3, TaskStatus::Todo),
            make_task_with_id(5, TaskStatus::Todo),
        ];
        let unblocked = super::compute_unblocked(&current_ready, &prev_ready_ids);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id(), TaskId(5));
    }

    #[test]
    fn compute_unblocked_empty_when_no_change() {
        let prev_ready_ids: HashSet<TaskId> = [TaskId(1), TaskId(2)].into_iter().collect();
        let current_ready = vec![
            make_task_with_id(1, TaskStatus::Todo),
            make_task_with_id(2, TaskStatus::Todo),
        ];
        let unblocked = super::compute_unblocked(&current_ready, &prev_ready_ids);
        assert!(unblocked.is_empty());
    }

    fn make_task_with_id(id: i64, status: TaskStatus) -> Task {
        Task::new(
            TaskId(id),
            ProjectId(1),
            format!("task-{id}"),
            None,
            None,
            None,
            Priority::P2,
            status,
            None,
            None,
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )
    }

    // --- is_ready / filter_ready / select_next tests ---

    fn make_task_with_opts(
        id: i64,
        priority: Priority,
        status: TaskStatus,
        created_at: &str,
    ) -> Task {
        Task::new(
            TaskId(id),
            ProjectId(1),
            format!("task-{id}"),
            None,
            None,
            None,
            priority,
            status,
            None,
            None,
            created_at.to_string(),
            created_at.to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )
    }

    // --- CompletionPolicy tests ---

    #[test]
    fn completion_policy_merge_mode_returns_none() {
        let policy = super::CompletionPolicy::new(MergeVia::Direct);
        let task = make_task(TaskStatus::InProgress);
        assert!(policy.required_pr_url(&task, false).unwrap().is_none());
    }

    #[test]
    fn completion_policy_pr_mode_no_url_errors() {
        let policy = super::CompletionPolicy::new(MergeVia::Pr);
        let task = make_task(TaskStatus::InProgress);
        assert!(policy.required_pr_url(&task, false).is_err());
    }

    #[test]
    fn completion_policy_pr_mode_with_url() {
        let policy = super::CompletionPolicy::new(MergeVia::Pr);
        let mut task = make_task(TaskStatus::InProgress);
        task.pr_url = Some("https://github.com/org/repo/pull/1".to_string());
        let result = policy.required_pr_url(&task, false).unwrap();
        assert_eq!(result, Some("https://github.com/org/repo/pull/1"));
    }

    #[test]
    fn completion_policy_skip_pr_check() {
        let policy = super::CompletionPolicy::new(MergeVia::Pr);
        let task = make_task(TaskStatus::InProgress);
        assert!(policy.required_pr_url(&task, true).unwrap().is_none());
    }

    // --- is_ready / filter_ready / select_next tests ---

    #[test]
    fn is_ready_todo_no_deps() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.is_ready(&HashMap::new()));
    }

    #[test]
    fn is_ready_draft_returns_false() {
        let task = make_task(TaskStatus::Draft);
        assert!(!task.is_ready(&HashMap::new()));
    }

    #[test]
    fn is_ready_in_progress_returns_false() {
        let task = make_task(TaskStatus::InProgress);
        assert!(!task.is_ready(&HashMap::new()));
    }

    #[test]
    fn is_ready_blocked_by_incomplete_dep() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[TaskId(10)], None).unwrap();
        let deps = HashMap::from([(TaskId(10), TaskStatus::Todo)]);
        assert!(!task.is_ready(&deps));
    }

    #[test]
    fn is_ready_blocked_by_in_progress_dep() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[TaskId(10)], None).unwrap();
        let deps = HashMap::from([(TaskId(10), TaskStatus::InProgress)]);
        assert!(!task.is_ready(&deps));
    }

    #[test]
    fn is_ready_unblocked_by_completed_dep() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[TaskId(10)], None).unwrap();
        let deps = HashMap::from([(TaskId(10), TaskStatus::Completed)]);
        assert!(task.is_ready(&deps));
    }

    #[test]
    fn is_ready_missing_dep_is_non_blocking() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[TaskId(99)], None).unwrap();
        assert!(task.is_ready(&HashMap::new()));
    }

    #[test]
    fn select_next_priority_order() {
        let tasks = vec![
            make_task_with_opts(1, Priority::P3, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
            make_task_with_opts(2, Priority::P0, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
            make_task_with_opts(3, Priority::P1, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), TaskId(2));
    }

    #[test]
    fn select_next_created_at_tiebreak() {
        let tasks = vec![
            make_task_with_opts(1, Priority::P2, TaskStatus::Todo, "2026-01-02T00:00:00Z"),
            make_task_with_opts(2, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), TaskId(2));
    }

    #[test]
    fn select_next_id_tiebreak() {
        let tasks = vec![
            make_task_with_opts(5, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
            make_task_with_opts(3, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), TaskId(3));
    }

    #[test]
    fn select_next_skips_blocked() {
        let t1 = make_task_with_opts(1, Priority::P0, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let (t1, _) = t1.set_dependencies(&[TaskId(10)], None).unwrap();
        let t2 = make_task_with_opts(2, Priority::P1, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let deps = HashMap::from([(TaskId(10), TaskStatus::InProgress)]);
        let result = super::select_next(vec![t1, t2], &deps).unwrap();
        assert_eq!(result.id(), TaskId(2));
    }

    #[test]
    fn select_next_empty() {
        assert!(super::select_next(vec![], &HashMap::new()).is_none());
    }

    #[test]
    fn select_next_skips_non_todo() {
        let tasks = vec![
            make_task_with_opts(1, Priority::P0, TaskStatus::Draft, "2026-01-01T00:00:00Z"),
            make_task_with_opts(
                2,
                Priority::P0,
                TaskStatus::InProgress,
                "2026-01-01T00:00:00Z",
            ),
            make_task_with_opts(3, Priority::P1, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), TaskId(3));
    }

    #[test]
    fn filter_ready_returns_only_eligible() {
        let t1 = make_task_with_opts(1, Priority::P0, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let t2 = make_task_with_opts(2, Priority::P1, TaskStatus::Draft, "2026-01-01T00:00:00Z");
        let t3 = make_task_with_opts(3, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let (t3, _) = t3.set_dependencies(&[TaskId(10)], None).unwrap();
        let deps = HashMap::from([(TaskId(10), TaskStatus::Todo)]);
        let ready = super::filter_ready(vec![t1, t2, t3], &deps);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id(), TaskId(1));
    }
}
