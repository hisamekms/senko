use serde::{Deserialize, Serialize};

use crate::application::{CompleteResult, PreviewResult};
use crate::infra::config::Config;
use crate::domain::project::Project;
use crate::domain::task::{DodItem, Task};
use crate::domain::user::{ApiKey, ApiKeyWithSecret, ProjectMember, User};

// --- Project ---

#[derive(Serialize)]
pub struct ProjectResponse {
    id: i64,
    name: String,
    description: Option<String>,
    created_at: String,
}

impl From<Project> for ProjectResponse {
    fn from(p: Project) -> Self {
        Self {
            id: p.id(),
            name: p.name().to_owned(),
            description: p.description().map(|s| s.to_owned()),
            created_at: p.created_at().to_owned(),
        }
    }
}

// --- Task ---

#[derive(Serialize)]
pub struct DodItemResponse {
    content: String,
    checked: bool,
}

impl From<&DodItem> for DodItemResponse {
    fn from(d: &DodItem) -> Self {
        Self {
            content: d.content().to_owned(),
            checked: d.checked(),
        }
    }
}

#[derive(Serialize)]
pub struct TaskResponse {
    id: i64,
    project_id: i64,
    title: String,
    background: Option<String>,
    description: Option<String>,
    plan: Option<String>,
    priority: String,
    status: String,
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
    definition_of_done: Vec<DodItemResponse>,
    in_scope: Vec<String>,
    out_of_scope: Vec<String>,
    tags: Vec<String>,
    dependencies: Vec<i64>,
}

impl From<Task> for TaskResponse {
    fn from(t: Task) -> Self {
        Self {
            id: t.id(),
            project_id: t.project_id(),
            title: t.title().to_owned(),
            background: t.background().map(|s| s.to_owned()),
            description: t.description().map(|s| s.to_owned()),
            plan: t.plan().map(|s| s.to_owned()),
            priority: t.priority().to_string(),
            status: t.status().to_string(),
            assignee_session_id: t.assignee_session_id().map(|s| s.to_owned()),
            assignee_user_id: t.assignee_user_id(),
            created_at: t.created_at().to_owned(),
            updated_at: t.updated_at().to_owned(),
            started_at: t.started_at().map(|s| s.to_owned()),
            completed_at: t.completed_at().map(|s| s.to_owned()),
            canceled_at: t.canceled_at().map(|s| s.to_owned()),
            cancel_reason: t.cancel_reason().map(|s| s.to_owned()),
            branch: t.branch().map(|s| s.to_owned()),
            pr_url: t.pr_url().map(|s| s.to_owned()),
            metadata: t.metadata().cloned(),
            definition_of_done: t.definition_of_done().iter().map(DodItemResponse::from).collect(),
            in_scope: t.in_scope().to_vec(),
            out_of_scope: t.out_of_scope().to_vec(),
            tags: t.tags().to_vec(),
            dependencies: t.dependencies().to_vec(),
        }
    }
}

// --- Task ViewModel (for web/HTML rendering) ---

pub struct DodItemViewModel {
    pub content: String,
    pub checked: bool,
}

pub struct TaskViewModel {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub canceled_at: Option<String>,
    pub cancel_reason: Option<String>,
    pub background: Option<String>,
    pub description: Option<String>,
    pub plan: Option<String>,
    pub assignee_session_id: Option<String>,
    pub assignee_user_id: Option<i64>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub definition_of_done: Vec<DodItemViewModel>,
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub dependencies: Vec<i64>,
}

impl From<Task> for TaskViewModel {
    fn from(t: Task) -> Self {
        Self {
            id: t.id(),
            title: t.title().to_owned(),
            status: t.status().to_string(),
            priority: t.priority().to_string(),
            tags: t.tags().to_vec(),
            created_at: t.created_at().to_owned(),
            updated_at: t.updated_at().to_owned(),
            started_at: t.started_at().map(|s| s.to_owned()),
            completed_at: t.completed_at().map(|s| s.to_owned()),
            canceled_at: t.canceled_at().map(|s| s.to_owned()),
            cancel_reason: t.cancel_reason().map(|s| s.to_owned()),
            background: t.background().map(|s| s.to_owned()),
            description: t.description().map(|s| s.to_owned()),
            plan: t.plan().map(|s| s.to_owned()),
            assignee_session_id: t.assignee_session_id().map(|s| s.to_owned()),
            assignee_user_id: t.assignee_user_id(),
            branch: t.branch().map(|s| s.to_owned()),
            pr_url: t.pr_url().map(|s| s.to_owned()),
            definition_of_done: t
                .definition_of_done()
                .iter()
                .map(|d| DodItemViewModel {
                    content: d.content().to_owned(),
                    checked: d.checked(),
                })
                .collect(),
            in_scope: t.in_scope().to_vec(),
            out_of_scope: t.out_of_scope().to_vec(),
            dependencies: t.dependencies().to_vec(),
        }
    }
}

// --- Complete Task ---

#[derive(Serialize)]
pub struct CompleteTaskResponse {
    pub task: TaskResponse,
    pub unblocked_tasks: Vec<UnblockedTaskInfo>,
}

impl From<CompleteResult> for CompleteTaskResponse {
    fn from(r: CompleteResult) -> Self {
        Self {
            task: TaskResponse::from(r.task),
            unblocked_tasks: r
                .unblocked
                .into_iter()
                .map(|t| UnblockedTaskInfo {
                    id: t.id(),
                    title: t.title().to_owned(),
                    status: "todo".to_owned(),
                    priority: t.priority().to_string(),
                })
                .collect(),
        }
    }
}

// --- Preview Transition ---

#[derive(Serialize, Deserialize)]
pub struct PreviewTransitionResponse {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub target_status: String,
    pub operations: Vec<String>,
    pub unblocked_tasks: Vec<UnblockedTaskInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct UnblockedTaskInfo {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub priority: String,
}

impl From<PreviewResult> for PreviewTransitionResponse {
    fn from(r: PreviewResult) -> Self {
        Self {
            allowed: r.allowed,
            reason: r.reason,
            target_status: r.target_status.to_string(),
            operations: r.operations,
            unblocked_tasks: r
                .unblocked_tasks
                .into_iter()
                .map(|t| UnblockedTaskInfo {
                    id: t.id(),
                    title: t.title().to_owned(),
                    status: t.status().to_string(),
                    priority: t.priority().to_string(),
                })
                .collect(),
        }
    }
}

// --- User ---

#[derive(Serialize)]
pub struct UserResponse {
    id: i64,
    username: String,
    display_name: Option<String>,
    email: Option<String>,
    created_at: String,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id(),
            username: u.username().to_owned(),
            display_name: u.display_name().map(|s| s.to_owned()),
            email: u.email().map(|s| s.to_owned()),
            created_at: u.created_at().to_owned(),
        }
    }
}

// --- ProjectMember ---

#[derive(Serialize)]
pub struct ProjectMemberResponse {
    id: i64,
    project_id: i64,
    user_id: i64,
    role: String,
    created_at: String,
}

impl From<ProjectMember> for ProjectMemberResponse {
    fn from(m: ProjectMember) -> Self {
        Self {
            id: m.id(),
            project_id: m.project_id(),
            user_id: m.user_id(),
            role: m.role().to_string(),
            created_at: m.created_at().to_owned(),
        }
    }
}

// --- ApiKey ---

#[derive(Serialize)]
pub struct ApiKeyResponse {
    id: i64,
    user_id: i64,
    key_prefix: String,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id(),
            user_id: k.user_id(),
            key_prefix: k.key_prefix().to_owned(),
            name: k.name().to_owned(),
            created_at: k.created_at().to_owned(),
            last_used_at: k.last_used_at().map(|s| s.to_owned()),
        }
    }
}

// --- ApiKeyWithSecret ---

#[derive(Serialize)]
pub struct ApiKeyWithSecretResponse {
    id: i64,
    user_id: i64,
    key: String,
    key_prefix: String,
    name: String,
    created_at: String,
}

impl From<ApiKeyWithSecret> for ApiKeyWithSecretResponse {
    fn from(k: ApiKeyWithSecret) -> Self {
        Self {
            id: k.id(),
            user_id: k.user_id(),
            key: k.key().to_owned(),
            key_prefix: k.key_prefix().to_owned(),
            name: k.name().to_owned(),
            created_at: k.created_at().to_owned(),
        }
    }
}

// --- Config ---

#[derive(Serialize)]
pub struct ConfigResponse(serde_json::Value);

impl From<Config> for ConfigResponse {
    fn from(c: Config) -> Self {
        Self(serde_json::to_value(c).unwrap_or_default())
    }
}
