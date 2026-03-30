use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMode {
    MergeThenComplete,
    PrThenComplete,
}

impl Default for CompletionMode {
    fn default() -> Self {
        CompletionMode::MergeThenComplete
    }
}

impl fmt::Display for CompletionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompletionMode::MergeThenComplete => write!(f, "merge_then_complete"),
            CompletionMode::PrThenComplete => write!(f, "pr_then_complete"),
        }
    }
}

/// Domain event emitted by Task aggregate methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskEvent {
    Created,
    Readied,
    Started,
    Completed,
    Canceled,
    DependencyAdded { dep_id: i64 },
    DependencyRemoved { dep_id: i64 },
    DependenciesSet { dep_ids: Vec<i64> },
    DodChecked { index: usize },
    DodUnchecked { index: usize },
}

/// A task that became eligible (ready) after another task was completed.
#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    id: i64,
    title: String,
    priority: Priority,
    metadata: Option<serde_json::Value>,
}

impl UnblockedTask {
    pub fn new(id: i64, title: String, priority: Priority, metadata: Option<serde_json::Value>) -> Self {
        Self { id, title, priority, metadata }
    }

    pub fn id(&self) -> i64 {
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
            _ => Err(DomainError::InvalidTaskStatus { value: s.to_string() }.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
            _ => Err(DomainError::InvalidPriority { value: value.to_string() }.into()),
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
            _ => Err(DomainError::InvalidPriority { value: s.to_string() }.into()),
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
    id: i64,
    project_id: i64,
    title: String,
    background: Option<String>,
    description: Option<String>,
    plan: Option<String>,
    priority: Priority,
    status: TaskStatus,
    assignee_session_id: Option<String>,
    assignee_user_id: Option<i64>,
    created_at: String,
    updated_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    canceled_at: Option<String>,
    cancel_reason: Option<String>,
    branch: Option<String>,
    pr_url: Option<String>,
    metadata: Option<serde_json::Value>,
    definition_of_done: Vec<DodItem>,
    in_scope: Vec<String>,
    out_of_scope: Vec<String>,
    tags: Vec<String>,
    dependencies: Vec<i64>,
}

impl Task {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i64,
        project_id: i64,
        title: String,
        background: Option<String>,
        description: Option<String>,
        plan: Option<String>,
        priority: Priority,
        status: TaskStatus,
        assignee_session_id: Option<String>,
        assignee_user_id: Option<i64>,
        created_at: String,
        updated_at: String,
        started_at: Option<String>,
        completed_at: Option<String>,
        canceled_at: Option<String>,
        cancel_reason: Option<String>,
        branch: Option<String>,
        pr_url: Option<String>,
        metadata: Option<serde_json::Value>,
        definition_of_done: Vec<DodItem>,
        in_scope: Vec<String>,
        out_of_scope: Vec<String>,
        tags: Vec<String>,
        dependencies: Vec<i64>,
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
            metadata,
            definition_of_done,
            in_scope,
            out_of_scope,
            tags,
            dependencies,
        }
    }

    // --- Getters ---

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn project_id(&self) -> i64 {
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

    pub fn assignee_user_id(&self) -> Option<i64> {
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

    pub fn dependencies(&self) -> &[i64] {
        &self.dependencies
    }

    // --- Query methods ---

    /// Returns true if this task is eligible for execution.
    ///
    /// A task is ready when:
    /// - status == Todo
    /// - All dependencies that exist in `dep_statuses` have status == Completed
    /// - Missing dependencies (not present in `dep_statuses`) are treated as non-blocking
    pub fn is_ready(&self, dep_statuses: &HashMap<i64, TaskStatus>) -> bool {
        self.status == TaskStatus::Todo
            && self.dependencies.iter().all(|dep_id| {
                dep_statuses
                    .get(dep_id)
                    .map_or(true, |s| *s == TaskStatus::Completed)
            })
    }

    // --- Update methods ---

    pub fn apply_update(mut self, params: &UpdateTaskParams, now: String) -> Self {
        let mut changed = false;
        if let Some(ref title) = params.title {
            self.title = title.clone();
            changed = true;
        }
        if let Some(ref background) = params.background {
            self.background = background.clone();
            changed = true;
        }
        if let Some(ref description) = params.description {
            self.description = description.clone();
            changed = true;
        }
        if let Some(ref plan) = params.plan {
            self.plan = plan.clone();
            changed = true;
        }
        if let Some(priority) = params.priority {
            self.priority = priority;
            changed = true;
        }
        if let Some(ref assignee_session_id) = params.assignee_session_id {
            self.assignee_session_id = assignee_session_id.clone();
            changed = true;
        }
        if let Some(ref assignee_user_id) = params.assignee_user_id {
            self.assignee_user_id = *assignee_user_id;
            changed = true;
        }
        if let Some(ref started_at) = params.started_at {
            self.started_at = started_at.clone();
            changed = true;
        }
        if let Some(ref completed_at) = params.completed_at {
            self.completed_at = completed_at.clone();
            changed = true;
        }
        if let Some(ref canceled_at) = params.canceled_at {
            self.canceled_at = canceled_at.clone();
            changed = true;
        }
        if let Some(ref cancel_reason) = params.cancel_reason {
            self.cancel_reason = cancel_reason.clone();
            changed = true;
        }
        if let Some(ref branch) = params.branch {
            self.branch = branch.clone();
            changed = true;
        }
        if let Some(ref pr_url) = params.pr_url {
            self.pr_url = pr_url.clone();
            changed = true;
        }
        if let Some(ref metadata) = params.metadata {
            self.metadata = metadata.clone();
            changed = true;
        }
        if changed {
            self.updated_at = now;
        }
        self
    }

    pub fn apply_array_update(mut self, params: &UpdateTaskArrayParams, now: String) -> Self {
        let mut changed = false;

        // Tags
        if let Some(ref set_tags) = params.set_tags {
            self.tags = set_tags.clone();
            changed = true;
        }
        if !params.add_tags.is_empty() {
            for tag in &params.add_tags {
                if !self.tags.contains(tag) {
                    self.tags.push(tag.clone());
                }
            }
            changed = true;
        }
        if !params.remove_tags.is_empty() {
            self.tags.retain(|t| !params.remove_tags.contains(t));
            changed = true;
        }

        // Definition of Done
        if let Some(ref set_dod) = params.set_definition_of_done {
            self.definition_of_done = set_dod
                .iter()
                .map(|c| DodItem::new(c.clone(), false))
                .collect();
            changed = true;
        }
        if !params.add_definition_of_done.is_empty() {
            for content in &params.add_definition_of_done {
                self.definition_of_done
                    .push(DodItem::new(content.clone(), false));
            }
            changed = true;
        }
        if !params.remove_definition_of_done.is_empty() {
            self.definition_of_done
                .retain(|d| !params.remove_definition_of_done.contains(&d.content));
            changed = true;
        }

        // In scope
        if let Some(ref set_in_scope) = params.set_in_scope {
            self.in_scope = set_in_scope.clone();
            changed = true;
        }
        if !params.add_in_scope.is_empty() {
            for item in &params.add_in_scope {
                if !self.in_scope.contains(item) {
                    self.in_scope.push(item.clone());
                }
            }
            changed = true;
        }
        if !params.remove_in_scope.is_empty() {
            self.in_scope
                .retain(|i| !params.remove_in_scope.contains(i));
            changed = true;
        }

        // Out of scope
        if let Some(ref set_out_of_scope) = params.set_out_of_scope {
            self.out_of_scope = set_out_of_scope.clone();
            changed = true;
        }
        if !params.add_out_of_scope.is_empty() {
            for item in &params.add_out_of_scope {
                if !self.out_of_scope.contains(item) {
                    self.out_of_scope.push(item.clone());
                }
            }
            changed = true;
        }
        if !params.remove_out_of_scope.is_empty() {
            self.out_of_scope
                .retain(|i| !params.remove_out_of_scope.contains(i));
            changed = true;
        }

        if changed {
            self.updated_at = now;
        }
        self
    }

    // --- Aggregate methods ---

    /// Transition: Draft -> Todo.
    pub fn ready(mut self, now: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::Todo)?;
        self.updated_at = now;
        Ok((self, vec![TaskEvent::Readied]))
    }

    /// Transition: Todo -> InProgress.
    pub fn start(mut self, assignee_session_id: Option<String>, assignee_user_id: Option<i64>, started_at: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::InProgress)?;
        self.assignee_session_id = assignee_session_id;
        self.assignee_user_id = assignee_user_id;
        self.updated_at = started_at.clone();
        self.started_at = Some(started_at);
        Ok((self, vec![TaskEvent::Started]))
    }

    /// Transition: InProgress -> Completed.
    ///
    /// Validates that all DoD items are checked before allowing completion.
    pub fn complete(mut self, completed_at: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        let unchecked_count = self.definition_of_done.iter().filter(|d| !d.checked).count();
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
    pub fn cancel(mut self, canceled_at: String, reason: Option<String>) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        self.status = self.status.transition_to(TaskStatus::Canceled)?;
        self.updated_at = canceled_at.clone();
        self.canceled_at = Some(canceled_at);
        self.cancel_reason = reason;
        Ok((self, vec![TaskEvent::Canceled]))
    }

    /// Add a dependency, validating self-dependency. Idempotent (no event if already present).
    pub fn add_dependency(mut self, dep_id: i64, now: Option<String>) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
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
    pub fn remove_dependency(mut self, dep_id: i64, now: Option<String>) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
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
    pub fn set_dependencies(mut self, dep_ids: &[i64], now: Option<String>) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        for &dep_id in dep_ids {
            if dep_id == self.id {
                return Err(DomainError::SelfDependency.into());
            }
        }
        self.dependencies = dep_ids.to_vec();
        if let Some(now) = now {
            self.updated_at = now;
        }
        Ok((self, vec![TaskEvent::DependenciesSet { dep_ids: dep_ids.to_vec() }]))
    }

    /// Check a DoD item by 1-based index.
    pub fn check_dod(mut self, index: usize, now: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id,
                count: self.definition_of_done.len(),
            }
            .into());
        }
        self.definition_of_done[index - 1].checked = true;
        self.updated_at = now;
        Ok((self, vec![TaskEvent::DodChecked { index }]))
    }

    /// Uncheck a DoD item by 1-based index.
    pub fn uncheck_dod(mut self, index: usize, now: String) -> anyhow::Result<(Task, Vec<TaskEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id,
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
pub fn filter_ready(tasks: Vec<Task>, dep_statuses: &HashMap<i64, TaskStatus>) -> Vec<Task> {
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
pub fn select_next(tasks: Vec<Task>, dep_statuses: &HashMap<i64, TaskStatus>) -> Option<Task> {
    let mut ready = filter_ready(tasks, dep_statuses);
    ready.sort_by(|a, b| {
        a.priority()
            .cmp(&b.priority())
            .then_with(|| a.created_at().cmp(b.created_at()))
            .then_with(|| a.id().cmp(&b.id()))
    });
    ready.into_iter().next()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<i64>,
}

#[derive(Clone)]
pub struct UpdateTaskParams {
    pub title: Option<String>,
    pub background: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub plan: Option<Option<String>>,
    pub priority: Option<Priority>,
    pub assignee_session_id: Option<Option<String>>,
    pub assignee_user_id: Option<Option<i64>>,
    pub started_at: Option<Option<String>>,
    pub completed_at: Option<Option<String>>,
    pub canceled_at: Option<Option<String>>,
    pub cancel_reason: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub pr_url: Option<Option<String>>,
    pub metadata: Option<Option<serde_json::Value>>,
}

#[derive(Clone)]
pub struct ListTasksFilter {
    pub statuses: Vec<TaskStatus>,
    pub tags: Vec<String>,
    pub depends_on: Option<i64>,
    pub ready: bool,
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

impl Default for ListTasksFilter {
    fn default() -> Self {
        Self {
            statuses: vec![],
            tags: vec![],
            depends_on: None,
            ready: false,
        }
    }
}

// --- Domain functions ---

/// Expand `${task_id}` placeholders in a branch template.
pub fn expand_branch_template(template: &str, task_id: i64) -> String {
    template.replace("${task_id}", &task_id.to_string())
}

/// Compute newly unblocked tasks by comparing current ready tasks against
/// the set of ready task IDs captured before a completion.
///
/// This is a pure function — the caller fetches ready tasks from the backend.
pub fn compute_unblocked(
    current_ready: &[Task],
    prev_ready_ids: &HashSet<i64>,
) -> Vec<UnblockedTask> {
    current_ready
        .iter()
        .filter(|t| !prev_ready_ids.contains(&t.id()))
        .map(|t| UnblockedTask::new(t.id(), t.title().to_string(), t.priority(), t.metadata().cloned()))
        .collect()
}

/// Domain policy for task completion workflow.
pub struct CompletionPolicy {
    completion_mode: CompletionMode,
    auto_merge: bool,
}

impl CompletionPolicy {
    pub fn new(completion_mode: CompletionMode, auto_merge: bool) -> Self {
        Self {
            completion_mode,
            auto_merge,
        }
    }

    pub fn auto_merge(&self) -> bool {
        self.auto_merge
    }

    /// Returns the PR URL that must be verified, or `None` if no PR check is needed.
    ///
    /// Returns `Err` if the completion mode requires a PR URL but none is set on the task.
    pub fn required_pr_url<'a>(&self, task: &'a Task, skip_pr_check: bool) -> Result<Option<&'a str>> {
        if skip_pr_check || self.completion_mode != CompletionMode::PrThenComplete {
            return Ok(None);
        }
        let pr_url = task.pr_url().ok_or_else(|| {
            anyhow::anyhow!(
                "cannot complete task #{}: completion_mode is pr_then_complete but no pr_url is set. \
                 Use `senko edit {} --pr-url <url>` to set it.",
                task.id(),
                task.id(),
            )
        })?;
        Ok(Some(pr_url))
    }
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
            assert!(from.transition_to(to).is_ok(), "{from} -> {to} should be ok");
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
            1, 1, "test".to_string(), None, None, None, Priority::P2, status,
            None, None,
            "2026-01-01T00:00:00Z".to_string(), "2026-01-01T00:00:00Z".to_string(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        )
    }

    #[test]
    fn task_ready_from_draft() {
        let task = make_task(TaskStatus::Draft);
        let (task, events) = task.ready("2026-01-02T00:00:00Z".to_string()).unwrap();
        assert_eq!(events, vec![TaskEvent::Readied]);
        assert_eq!(task.status(), TaskStatus::Todo);
        assert_eq!(task.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn task_ready_from_todo_fails() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.ready("2026-01-02T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_start_from_todo() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task.start(Some("session-1".to_string()), None, "2026-01-02T00:00:00Z".to_string()).unwrap();
        assert_eq!(events, vec![TaskEvent::Started]);
        assert_eq!(task.status(), TaskStatus::InProgress);
        assert_eq!(task.assignee_session_id(), Some("session-1"));
        assert_eq!(task.started_at(), Some("2026-01-02T00:00:00Z"));
        assert_eq!(task.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn task_start_from_draft_fails() {
        let task = make_task(TaskStatus::Draft);
        assert!(task.start(None, None, "2026-01-02T00:00:00Z".to_string()).is_err());
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
        let err = task.complete("2026-01-03T00:00:00Z".to_string()).unwrap_err();
        assert!(err.to_string().contains("unchecked DoD item(s)"));
    }

    #[test]
    fn task_complete_with_all_dod_checked() {
        let task = make_task_with_dod();
        let (task, _) = task.check_dod(1, "2026-01-03T00:00:00Z".to_string()).unwrap();
        let (task, _) = task.check_dod(2, "2026-01-03T00:00:00Z".to_string()).unwrap();
        let (task, _) = task.complete("2026-01-03T00:00:00Z".to_string()).unwrap();
        assert_eq!(task.status(), TaskStatus::Completed);
    }

    #[test]
    fn task_cancel_from_draft() {
        let task = make_task(TaskStatus::Draft);
        let (task, events) = task.cancel("2026-01-04T00:00:00Z".to_string(), Some("not needed".to_string())).unwrap();
        assert_eq!(events, vec![TaskEvent::Canceled]);
        assert_eq!(task.status(), TaskStatus::Canceled);
        assert_eq!(task.canceled_at(), Some("2026-01-04T00:00:00Z"));
        assert_eq!(task.cancel_reason(), Some("not needed"));
        assert_eq!(task.updated_at(), "2026-01-04T00:00:00Z");
    }

    #[test]
    fn task_cancel_from_in_progress() {
        let task = make_task(TaskStatus::InProgress);
        let (task, events) = task.cancel("2026-01-04T00:00:00Z".to_string(), None).unwrap();
        assert_eq!(events, vec![TaskEvent::Canceled]);
        assert_eq!(task.status(), TaskStatus::Canceled);
        assert_eq!(task.updated_at(), "2026-01-04T00:00:00Z");
    }

    #[test]
    fn task_cancel_from_completed_fails() {
        let task = make_task(TaskStatus::Completed);
        assert!(task.cancel("2026-01-04T00:00:00Z".to_string(), None).is_err());
    }

    // --- Dependency management tests ---

    #[test]
    fn task_add_dependency() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task.add_dependency(2, None).unwrap();
        assert_eq!(task.dependencies(), &[2]);
        assert_eq!(events, vec![TaskEvent::DependencyAdded { dep_id: 2 }]);
    }

    #[test]
    fn task_add_dependency_self_error() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.add_dependency(1, None).is_err());
    }

    #[test]
    fn task_add_dependency_idempotent() {
        let task = make_task(TaskStatus::Todo);
        let (task, events) = task.add_dependency(2, None).unwrap();
        assert_eq!(events.len(), 1);
        let (task, events) = task.add_dependency(2, None).unwrap();
        assert!(events.is_empty());
        assert_eq!(task.dependencies(), &[2]);
    }

    #[test]
    fn task_remove_dependency() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.add_dependency(2, None).unwrap();
        let (task, _) = task.add_dependency(3, None).unwrap();
        let (task, events) = task.remove_dependency(2, None).unwrap();
        assert_eq!(task.dependencies(), &[3]);
        assert_eq!(events, vec![TaskEvent::DependencyRemoved { dep_id: 2 }]);
    }

    #[test]
    fn task_remove_dependency_not_found() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.remove_dependency(99, None).is_err());
    }

    #[test]
    fn task_set_dependencies() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.add_dependency(2, None).unwrap();
        let (task, events) = task.set_dependencies(&[3, 4], None).unwrap();
        assert_eq!(task.dependencies(), &[3, 4]);
        assert_eq!(events, vec![TaskEvent::DependenciesSet { dep_ids: vec![3, 4] }]);
    }

    #[test]
    fn task_set_dependencies_self_error() {
        let task = make_task(TaskStatus::Todo);
        assert!(task.set_dependencies(&[1, 2], None).is_err());
    }

    // --- DoD operation tests ---

    fn make_task_with_dod() -> Task {
        Task::new(
            1, 1, "test".to_string(), None, None, None, Priority::P2, TaskStatus::InProgress,
            None, None,
            "2026-01-01T00:00:00Z".to_string(), "2026-01-01T00:00:00Z".to_string(),
            None, None, None, None, None, None, None,
            vec![
                DodItem::new("Write tests".to_string(), false),
                DodItem::new("Update docs".to_string(), false),
            ],
            vec![], vec![], vec![], vec![],
        )
    }

    #[test]
    fn task_check_dod() {
        let task = make_task_with_dod();
        let (task, events) = task.check_dod(1, "2026-01-05T00:00:00Z".to_string()).unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());
        assert_eq!(task.updated_at(), "2026-01-05T00:00:00Z");
        assert_eq!(events, vec![TaskEvent::DodChecked { index: 1 }]);
    }

    #[test]
    fn task_uncheck_dod() {
        let task = Task::new(
            1, 1, "test".to_string(), None, None, None, Priority::P2, TaskStatus::InProgress,
            None, None,
            "2026-01-01T00:00:00Z".to_string(), "2026-01-01T00:00:00Z".to_string(),
            None, None, None, None, None, None, None,
            vec![
                DodItem::new("Write tests".to_string(), true),
                DodItem::new("Update docs".to_string(), false),
            ],
            vec![], vec![], vec![], vec![],
        );
        let (task, events) = task.uncheck_dod(1, "2026-01-05T00:00:00Z".to_string()).unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert_eq!(task.updated_at(), "2026-01-05T00:00:00Z");
        assert_eq!(events, vec![TaskEvent::DodUnchecked { index: 1 }]);
    }

    #[test]
    fn task_check_dod_index_zero() {
        let task = make_task_with_dod();
        assert!(task.check_dod(0, "2026-01-05T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_check_dod_index_out_of_range() {
        let task = make_task_with_dod();
        assert!(task.check_dod(3, "2026-01-05T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_check_dod_empty_list() {
        let task = make_task(TaskStatus::InProgress);
        assert!(task.check_dod(1, "2026-01-05T00:00:00Z".to_string()).is_err());
    }

    // --- expand_branch_template tests ---

    #[test]
    fn expand_branch_template_replaces_task_id() {
        assert_eq!(
            super::expand_branch_template("feature/${task_id}-impl", 42),
            "feature/42-impl"
        );
    }

    #[test]
    fn expand_branch_template_no_placeholder() {
        assert_eq!(
            super::expand_branch_template("feature/my-branch", 42),
            "feature/my-branch"
        );
    }

    #[test]
    fn expand_branch_template_multiple_placeholders() {
        assert_eq!(
            super::expand_branch_template("${task_id}/${task_id}", 7),
            "7/7"
        );
    }

    // --- compute_unblocked tests ---

    #[test]
    fn compute_unblocked_finds_newly_ready() {
        let prev_ready_ids: HashSet<i64> = [1, 3].into_iter().collect();
        let current_ready = vec![
            make_task_with_id(1, TaskStatus::Todo),
            make_task_with_id(3, TaskStatus::Todo),
            make_task_with_id(5, TaskStatus::Todo),
        ];
        let unblocked = super::compute_unblocked(&current_ready, &prev_ready_ids);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id(), 5);
    }

    #[test]
    fn compute_unblocked_empty_when_no_change() {
        let prev_ready_ids: HashSet<i64> = [1, 2].into_iter().collect();
        let current_ready = vec![
            make_task_with_id(1, TaskStatus::Todo),
            make_task_with_id(2, TaskStatus::Todo),
        ];
        let unblocked = super::compute_unblocked(&current_ready, &prev_ready_ids);
        assert!(unblocked.is_empty());
    }

    fn make_task_with_id(id: i64, status: TaskStatus) -> Task {
        Task::new(
            id, 1, format!("task-{id}"), None, None, None, Priority::P2, status,
            None, None,
            "2026-01-01T00:00:00Z".to_string(), "2026-01-01T00:00:00Z".to_string(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        )
    }

    // --- is_ready / filter_ready / select_next tests ---

    fn make_task_with_opts(id: i64, priority: Priority, status: TaskStatus, created_at: &str) -> Task {
        Task::new(
            id, 1, format!("task-{id}"), None, None, None, priority, status,
            None, None,
            created_at.to_string(), created_at.to_string(),
            None, None, None, None, None, None, None,
            vec![], vec![], vec![], vec![], vec![],
        )
    }

    // --- CompletionPolicy tests ---

    #[test]
    fn completion_policy_merge_mode_returns_none() {
        let policy = super::CompletionPolicy::new(CompletionMode::MergeThenComplete, true);
        let task = make_task(TaskStatus::InProgress);
        assert!(policy.required_pr_url(&task, false).unwrap().is_none());
    }

    #[test]
    fn completion_policy_pr_mode_no_url_errors() {
        let policy = super::CompletionPolicy::new(CompletionMode::PrThenComplete, true);
        let task = make_task(TaskStatus::InProgress);
        assert!(policy.required_pr_url(&task, false).is_err());
    }

    #[test]
    fn completion_policy_pr_mode_with_url() {
        let policy = super::CompletionPolicy::new(CompletionMode::PrThenComplete, true);
        let mut task = make_task(TaskStatus::InProgress);
        task.pr_url = Some("https://github.com/org/repo/pull/1".to_string());
        let result = policy.required_pr_url(&task, false).unwrap();
        assert_eq!(result, Some("https://github.com/org/repo/pull/1"));
    }

    #[test]
    fn completion_policy_skip_pr_check() {
        let policy = super::CompletionPolicy::new(CompletionMode::PrThenComplete, true);
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
        let (task, _) = task.set_dependencies(&[10], None).unwrap();
        let deps = HashMap::from([(10, TaskStatus::Todo)]);
        assert!(!task.is_ready(&deps));
    }

    #[test]
    fn is_ready_blocked_by_in_progress_dep() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[10], None).unwrap();
        let deps = HashMap::from([(10, TaskStatus::InProgress)]);
        assert!(!task.is_ready(&deps));
    }

    #[test]
    fn is_ready_unblocked_by_completed_dep() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[10], None).unwrap();
        let deps = HashMap::from([(10, TaskStatus::Completed)]);
        assert!(task.is_ready(&deps));
    }

    #[test]
    fn is_ready_missing_dep_is_non_blocking() {
        let task = make_task(TaskStatus::Todo);
        let (task, _) = task.set_dependencies(&[99], None).unwrap();
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
        assert_eq!(result.id(), 2);
    }

    #[test]
    fn select_next_created_at_tiebreak() {
        let tasks = vec![
            make_task_with_opts(1, Priority::P2, TaskStatus::Todo, "2026-01-02T00:00:00Z"),
            make_task_with_opts(2, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), 2);
    }

    #[test]
    fn select_next_id_tiebreak() {
        let tasks = vec![
            make_task_with_opts(5, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
            make_task_with_opts(3, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), 3);
    }

    #[test]
    fn select_next_skips_blocked() {
        let t1 = make_task_with_opts(1, Priority::P0, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let (t1, _) = t1.set_dependencies(&[10], None).unwrap();
        let t2 = make_task_with_opts(2, Priority::P1, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let deps = HashMap::from([(10, TaskStatus::InProgress)]);
        let result = super::select_next(vec![t1, t2], &deps).unwrap();
        assert_eq!(result.id(), 2);
    }

    #[test]
    fn select_next_empty() {
        assert!(super::select_next(vec![], &HashMap::new()).is_none());
    }

    #[test]
    fn select_next_skips_non_todo() {
        let tasks = vec![
            make_task_with_opts(1, Priority::P0, TaskStatus::Draft, "2026-01-01T00:00:00Z"),
            make_task_with_opts(2, Priority::P0, TaskStatus::InProgress, "2026-01-01T00:00:00Z"),
            make_task_with_opts(3, Priority::P1, TaskStatus::Todo, "2026-01-01T00:00:00Z"),
        ];
        let result = super::select_next(tasks, &HashMap::new()).unwrap();
        assert_eq!(result.id(), 3);
    }

    #[test]
    fn filter_ready_returns_only_eligible() {
        let t1 = make_task_with_opts(1, Priority::P0, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let t2 = make_task_with_opts(2, Priority::P1, TaskStatus::Draft, "2026-01-01T00:00:00Z");
        let t3 = make_task_with_opts(3, Priority::P2, TaskStatus::Todo, "2026-01-01T00:00:00Z");
        let (t3, _) = t3.set_dependencies(&[10], None).unwrap();
        let deps = HashMap::from([(10, TaskStatus::Todo)]);
        let ready = super::filter_ready(vec![t1, t2, t3], &deps);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id(), 1);
    }
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task>;
    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()>;
    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()>;
    async fn add_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn remove_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn set_dependencies(&self, project_id: i64, task_id: i64, dep_ids: &[i64]) -> Result<Task>;
    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>>;
    async fn save(&self, task: &Task) -> Result<()>;
}
