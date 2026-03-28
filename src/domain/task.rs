use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// A task that became eligible (ready) after another task was completed.
#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    pub id: i64,
    pub title: String,
    pub priority: Priority,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
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
            Err(anyhow::anyhow!(
                "invalid status transition: {} -> {}",
                self,
                to
            ))
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
            _ => Err(anyhow::anyhow!("invalid task status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
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
            _ => Err(anyhow::anyhow!("invalid priority: {value}")),
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
            _ => Err(anyhow::anyhow!("invalid priority: {s} (expected p0-p3)")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DodItem {
    pub content: String,
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub background: Option<String>,
    pub description: Option<String>,
    pub plan: Option<String>,
    pub priority: Priority,
    pub status: TaskStatus,
    pub assignee_session_id: Option<String>,
    pub assignee_user_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub canceled_at: Option<String>,
    pub cancel_reason: Option<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub definition_of_done: Vec<DodItem>,
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub tags: Vec<String>,
    pub dependencies: Vec<i64>,
}

impl Task {
    /// Transition: Draft -> Todo
    pub fn ready(&mut self) -> anyhow::Result<()> {
        self.status = self.status.transition_to(TaskStatus::Todo)?;
        Ok(())
    }

    /// Transition: Todo -> InProgress
    pub fn start(&mut self, assignee_session_id: Option<String>, assignee_user_id: Option<i64>, started_at: String) -> anyhow::Result<()> {
        self.status = self.status.transition_to(TaskStatus::InProgress)?;
        self.assignee_session_id = assignee_session_id;
        self.assignee_user_id = assignee_user_id;
        self.started_at = Some(started_at);
        Ok(())
    }

    /// Transition: InProgress -> Completed
    pub fn complete(&mut self, completed_at: String) -> anyhow::Result<()> {
        self.status = self.status.transition_to(TaskStatus::Completed)?;
        self.completed_at = Some(completed_at);
        Ok(())
    }

    /// Transition: active -> Canceled
    pub fn cancel(&mut self, canceled_at: String, reason: Option<String>) -> anyhow::Result<()> {
        self.status = self.status.transition_to(TaskStatus::Canceled)?;
        self.canceled_at = Some(canceled_at);
        self.cancel_reason = reason;
        Ok(())
    }

    /// Add a dependency, validating self-dependency. Idempotent.
    pub fn add_dependency(&mut self, dep_id: i64) -> anyhow::Result<()> {
        if self.id == dep_id {
            anyhow::bail!("a task cannot depend on itself");
        }
        if !self.dependencies.contains(&dep_id) {
            self.dependencies.push(dep_id);
        }
        Ok(())
    }

    /// Remove a dependency, validating existence.
    pub fn remove_dependency(&mut self, dep_id: i64) -> anyhow::Result<()> {
        let before = self.dependencies.len();
        self.dependencies.retain(|&d| d != dep_id);
        if self.dependencies.len() == before {
            anyhow::bail!("dependency not found: task {} does not depend on {}", self.id, dep_id);
        }
        Ok(())
    }

    /// Replace all dependencies, validating no self-dependency.
    pub fn set_dependencies(&mut self, dep_ids: &[i64]) -> anyhow::Result<()> {
        for &dep_id in dep_ids {
            if dep_id == self.id {
                anyhow::bail!("a task cannot depend on itself");
            }
        }
        self.dependencies = dep_ids.to_vec();
        Ok(())
    }

    /// Check a DoD item by 1-based index.
    pub fn check_dod(&mut self, index: usize) -> anyhow::Result<()> {
        if index == 0 || index > self.definition_of_done.len() {
            anyhow::bail!(
                "DoD index {} out of range (task #{} has {} DoD item(s))",
                index, self.id, self.definition_of_done.len()
            );
        }
        self.definition_of_done[index - 1].checked = true;
        Ok(())
    }

    /// Uncheck a DoD item by 1-based index.
    pub fn uncheck_dod(&mut self, index: usize) -> anyhow::Result<()> {
        if index == 0 || index > self.definition_of_done.len() {
            anyhow::bail!(
                "DoD index {} out of range (task #{} has {} DoD item(s))",
                index, self.id, self.definition_of_done.len()
            );
        }
        self.definition_of_done[index - 1].checked = false;
        Ok(())
    }
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
        Task {
            id: 1,
            project_id: 1,
            title: "test".to_string(),
            background: None,
            description: None,
            plan: None,
            priority: Priority::P2,
            status,
            assignee_session_id: None,
            assignee_user_id: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn task_ready_from_draft() {
        let mut task = make_task(TaskStatus::Draft);
        assert!(task.ready().is_ok());
        assert_eq!(task.status, TaskStatus::Todo);
    }

    #[test]
    fn task_ready_from_todo_fails() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.ready().is_err());
    }

    #[test]
    fn task_start_from_todo() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.start(Some("session-1".to_string()), None, "2026-01-02T00:00:00Z".to_string()).is_ok());
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.assignee_session_id.as_deref(), Some("session-1"));
        assert_eq!(task.started_at.as_deref(), Some("2026-01-02T00:00:00Z"));
    }

    #[test]
    fn task_start_from_draft_fails() {
        let mut task = make_task(TaskStatus::Draft);
        assert!(task.start(None, None, "2026-01-02T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_complete_from_in_progress() {
        let mut task = make_task(TaskStatus::InProgress);
        assert!(task.complete("2026-01-03T00:00:00Z".to_string()).is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.completed_at.as_deref(), Some("2026-01-03T00:00:00Z"));
    }

    #[test]
    fn task_complete_from_todo_fails() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.complete("2026-01-03T00:00:00Z".to_string()).is_err());
    }

    #[test]
    fn task_cancel_from_draft() {
        let mut task = make_task(TaskStatus::Draft);
        assert!(task.cancel("2026-01-04T00:00:00Z".to_string(), Some("not needed".to_string())).is_ok());
        assert_eq!(task.status, TaskStatus::Canceled);
        assert_eq!(task.canceled_at.as_deref(), Some("2026-01-04T00:00:00Z"));
        assert_eq!(task.cancel_reason.as_deref(), Some("not needed"));
    }

    #[test]
    fn task_cancel_from_in_progress() {
        let mut task = make_task(TaskStatus::InProgress);
        assert!(task.cancel("2026-01-04T00:00:00Z".to_string(), None).is_ok());
        assert_eq!(task.status, TaskStatus::Canceled);
    }

    #[test]
    fn task_cancel_from_completed_fails() {
        let mut task = make_task(TaskStatus::Completed);
        assert!(task.cancel("2026-01-04T00:00:00Z".to_string(), None).is_err());
    }

    // --- Dependency management tests ---

    #[test]
    fn task_add_dependency() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.add_dependency(2).is_ok());
        assert_eq!(task.dependencies, vec![2]);
    }

    #[test]
    fn task_add_dependency_self_error() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.add_dependency(1).is_err());
    }

    #[test]
    fn task_add_dependency_idempotent() {
        let mut task = make_task(TaskStatus::Todo);
        task.add_dependency(2).unwrap();
        task.add_dependency(2).unwrap();
        assert_eq!(task.dependencies, vec![2]);
    }

    #[test]
    fn task_remove_dependency() {
        let mut task = make_task(TaskStatus::Todo);
        task.dependencies = vec![2, 3];
        assert!(task.remove_dependency(2).is_ok());
        assert_eq!(task.dependencies, vec![3]);
    }

    #[test]
    fn task_remove_dependency_not_found() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.remove_dependency(99).is_err());
    }

    #[test]
    fn task_set_dependencies() {
        let mut task = make_task(TaskStatus::Todo);
        task.dependencies = vec![2];
        assert!(task.set_dependencies(&[3, 4]).is_ok());
        assert_eq!(task.dependencies, vec![3, 4]);
    }

    #[test]
    fn task_set_dependencies_self_error() {
        let mut task = make_task(TaskStatus::Todo);
        assert!(task.set_dependencies(&[1, 2]).is_err());
    }

    // --- DoD operation tests ---

    fn make_task_with_dod() -> Task {
        let mut task = make_task(TaskStatus::InProgress);
        task.definition_of_done = vec![
            DodItem { content: "Write tests".to_string(), checked: false },
            DodItem { content: "Update docs".to_string(), checked: false },
        ];
        task
    }

    #[test]
    fn task_check_dod() {
        let mut task = make_task_with_dod();
        assert!(task.check_dod(1).is_ok());
        assert!(task.definition_of_done[0].checked);
        assert!(!task.definition_of_done[1].checked);
    }

    #[test]
    fn task_uncheck_dod() {
        let mut task = make_task_with_dod();
        task.definition_of_done[0].checked = true;
        assert!(task.uncheck_dod(1).is_ok());
        assert!(!task.definition_of_done[0].checked);
    }

    #[test]
    fn task_check_dod_index_zero() {
        let mut task = make_task_with_dod();
        assert!(task.check_dod(0).is_err());
    }

    #[test]
    fn task_check_dod_index_out_of_range() {
        let mut task = make_task_with_dod();
        assert!(task.check_dod(3).is_err());
    }

    #[test]
    fn task_check_dod_empty_list() {
        let mut task = make_task(TaskStatus::InProgress);
        assert!(task.check_dod(1).is_err());
    }
}
