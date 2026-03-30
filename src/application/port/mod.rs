pub mod hook_executor;
pub mod pr_verifier;
pub mod task_query;

pub use hook_executor::{HookExecutor, NoOpHookExecutor};
pub use pr_verifier::{NoOpPrVerifier, PrVerifier};
pub use task_query::TaskQueryPort;

use crate::domain::repository::{ProjectRepository, TaskRepository};

/// Combined trait for backends that implement TaskRepository, ProjectRepository, and TaskQueryPort.
/// Backends automatically implement TaskBackend via the blanket impl.
pub trait TaskBackend: TaskRepository + ProjectRepository + TaskQueryPort {}

impl<T: TaskRepository + ProjectRepository + TaskQueryPort> TaskBackend for T {}
