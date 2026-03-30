pub mod auth;
pub mod authentication;
pub mod hook_executor;
pub mod hook_test;
pub mod pr_verifier;
pub mod project_query;
pub mod task_operations;
pub mod task_query;
pub mod task_transition;
pub mod user_query;

pub use auth::{AuthError, AuthProvider};
pub use authentication::AuthenticationPort;
pub use hook_executor::{HookExecutor, NoOpHookExecutor};
pub use hook_test::HookTestPort;
pub use pr_verifier::{NoOpPrVerifier, PrVerifier};
pub use project_query::ProjectQueryPort;
pub use task_operations::{PreviewResult, TaskOperations};
pub use task_query::TaskQueryPort;
pub use task_transition::TaskTransitionPort;
pub use user_query::UserQueryPort;

use crate::domain::{ApiKeyRepository, ProjectMemberRepository, ProjectRepository, TaskRepository, UserRepository};

/// Combined trait for backends that implement all repository traits, query ports, and TaskTransitionPort.
/// Backends automatically implement TaskBackend via the blanket impl.
pub trait TaskBackend: TaskRepository + ProjectRepository + ProjectMemberRepository + UserRepository + ApiKeyRepository + AuthenticationPort + TaskQueryPort + TaskTransitionPort + ProjectQueryPort + UserQueryPort {}

impl<T: TaskRepository + ProjectRepository + ProjectMemberRepository + UserRepository + ApiKeyRepository + AuthenticationPort + TaskQueryPort + TaskTransitionPort + ProjectQueryPort + UserQueryPort> TaskBackend for T {}
