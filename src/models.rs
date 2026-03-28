use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

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
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectParams {
    pub name: String,
    pub description: Option<String>,
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
}
