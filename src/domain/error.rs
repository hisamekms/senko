use super::task::TaskId;

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
    DependencyNotFound { task_id: TaskId, dep_id: TaskId },

    #[error("metadata field not found")]
    MetadataFieldNotFound,

    #[error("contract not found")]
    ContractNotFound,

    #[error("no eligible task found")]
    NoEligibleTask,

    #[error("aborted by pre-hook for event {event}")]
    HookAborted { event: String },

    // BadRequest (400)
    #[error("invalid task status: {value}")]
    InvalidTaskStatus { value: String },

    #[error("invalid priority: {value}")]
    InvalidPriority { value: String },

    #[error("invalid role: {value}")]
    InvalidRole { value: String },

    #[error("invalid verification type: {value} (expected static, execution, or manual)")]
    InvalidVerificationType { value: String },

    #[error("a task cannot depend on itself")]
    SelfDependency,

    #[error("adding dependency on {dep_id} would create a cycle")]
    DependencyCycle { dep_id: TaskId },

    #[error("DoD index {index} out of range (task #{task_id} has {count} DoD item(s))")]
    DodIndexOutOfRange {
        index: usize,
        task_id: i64,
        count: usize,
    },

    #[error("metadata too large: {size} bytes exceeds {max} byte limit")]
    MetadataTooLarge { size: usize, max: usize },

    #[error("metadata nesting too deep: depth {depth} exceeds maximum of {max}")]
    MetadataTooDeep { depth: u32, max: u32 },

    #[error("invalid metadata field type: {value}")]
    InvalidMetadataFieldType { value: String },

    #[error("invalid metadata field name: {reason}")]
    InvalidMetadataFieldName { reason: String },

    #[error("invalid cursor")]
    InvalidCursor,

    #[error("cursor does not match {dimension}: expected {expected}, got {got}")]
    CursorMismatch {
        /// Which axis of the cursor mismatched the request: `"order_by"` for
        /// `kind` (id / updated_at / priority) mismatch, `"order"` for
        /// direction (asc / desc) mismatch.
        dimension: &'static str,
        expected: &'static str,
        got: &'static str,
    },

    #[error("invalid query parameter '{field}': {value}")]
    InvalidQueryParam { field: &'static str, value: String },

    #[error("{message}")]
    ValidationError { field: String, message: String },

    // Conflict (409)
    #[error("invalid status transition: {from} -> {to}")]
    InvalidStatusTransition { from: String, to: String },

    #[error("cannot complete task #{task_id}: {reason}")]
    CannotCompleteTask { task_id: TaskId, reason: String },

    #[error("cannot delete the default project")]
    CannotDeleteDefaultProject,

    #[error("cannot delete project with {count} existing task(s)")]
    CannotDeleteProjectWithTasks { count: i64 },

    #[error("session limit exceeded: maximum {max} active sessions per user")]
    SessionLimitExceeded { max: u32 },

    #[error("metadata field name already exists in this project: {name}")]
    MetadataFieldNameConflict { name: String },

    // Not Implemented (501)
    #[error("operation not supported: {operation}")]
    UnsupportedOperation { operation: String },
}
