pub mod auth;
pub mod authentication;
pub mod hook_executor;
pub mod hook_test;
pub mod pr_verifier;
pub mod task_query;
pub mod task_transition;

pub use auth::{AuthError, AuthProvider};
pub use authentication::AuthenticationPort;
pub use hook_executor::{HookExecutor, NoOpHookExecutor};
pub use hook_test::HookTestPort;
pub use pr_verifier::{NoOpPrVerifier, PrVerifier};
pub use task_query::TaskQueryPort;
pub use task_transition::TaskTransitionPort;

use crate::domain::{ApiKeyRepository, ProjectRepository, TaskRepository, UserRepository};

/// Combined trait for backends that implement all repository traits, TaskQueryPort, and TaskTransitionPort.
/// Backends automatically implement TaskBackend via the blanket impl.
pub trait TaskBackend: TaskRepository + ProjectRepository + UserRepository + ApiKeyRepository + AuthenticationPort + TaskQueryPort + TaskTransitionPort {}

impl<T: TaskRepository + ProjectRepository + UserRepository + ApiKeyRepository + AuthenticationPort + TaskQueryPort + TaskTransitionPort> TaskBackend for T {}
