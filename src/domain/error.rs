#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    // NotFound (404)
    #[error("task not found")]
    TaskNotFound,

    #[error("project not found")]
    ProjectNotFound,

    #[error("user not found")]
    UserNotFound,

    #[error("project member not found")]
    ProjectMemberNotFound,

    #[error("api key not found")]
    ApiKeyNotFound,

    #[error("dependency not found: task {task_id} does not depend on {dep_id}")]
    DependencyNotFound { task_id: i64, dep_id: i64 },

    #[error("no eligible task found")]
    NoEligibleTask,

    // BadRequest (400)
    #[error("invalid task status: {value}")]
    InvalidTaskStatus { value: String },

    #[error("invalid priority: {value}")]
    InvalidPriority { value: String },

    #[error("invalid role: {value}")]
    InvalidRole { value: String },

    #[error("a task cannot depend on itself")]
    SelfDependency,

    #[error("adding dependency on {dep_id} would create a cycle")]
    DependencyCycle { dep_id: i64 },

    #[error("DoD index {index} out of range (task #{task_id} has {count} DoD item(s))")]
    DodIndexOutOfRange {
        index: usize,
        task_id: i64,
        count: usize,
    },

    // Conflict (409)
    #[error("invalid status transition: {from} -> {to}")]
    InvalidStatusTransition { from: String, to: String },

    #[error("cannot complete task #{task_id}: {reason}")]
    CannotCompleteTask { task_id: i64, reason: String },

    #[error("cannot delete the default project")]
    CannotDeleteDefaultProject,

    #[error("cannot delete project with {count} existing task(s)")]
    CannotDeleteProjectWithTasks { count: i64 },
}
