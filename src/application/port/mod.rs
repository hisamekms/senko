pub mod auth;
pub mod authentication;
pub mod contract_operations;
pub mod hook_data_source;
pub mod hook_executor;
pub mod hook_test;
pub mod metadata_field_operations;
pub mod pr_verifier;
pub mod project_operations;
pub mod project_query;
#[cfg(feature = "dev-tools")]
pub mod seeder;
pub mod task_operations;
pub mod task_query;
pub mod task_transition;
pub mod user_operations;
pub mod user_query;

pub use auth::{AuthError, AuthProvider};
pub use authentication::{ApiKeyAuthResult, AuthenticationPort};
pub use contract_operations::ContractOperations;
pub use hook_data_source::{BackendHookData, HookDataSource};
pub use hook_executor::{HookExecutor, NoOpHookExecutor};
pub use hook_test::HookTestPort;
pub use metadata_field_operations::MetadataFieldOperations;
pub use pr_verifier::PrVerifier;
pub use project_operations::ProjectOperations;
pub use project_query::ProjectQueryPort;
#[cfg(feature = "dev-tools")]
pub use seeder::SeederPort;
pub use task_operations::{CompleteResult, PreviewResult, TaskOperations};
pub use task_query::TaskQueryPort;
pub use task_transition::TaskTransitionPort;
pub use user_operations::UserOperations;
pub use user_query::UserQueryPort;

use crate::domain::contract::ContractRepository;
use crate::domain::{
    ApiKeyRepository, MetadataFieldRepository, ProjectMemberRepository, ProjectRepository,
    TaskRepository, UserRepository,
};

/// Combined trait for backends that implement all repository traits, query ports, and TaskTransitionPort.
/// Backends automatically implement TaskBackend via the blanket impl.
pub trait TaskBackend:
    TaskRepository
    + ContractRepository
    + ProjectRepository
    + ProjectMemberRepository
    + UserRepository
    + ApiKeyRepository
    + MetadataFieldRepository
    + AuthenticationPort
    + TaskQueryPort
    + TaskTransitionPort
    + ProjectQueryPort
    + UserQueryPort
{
}

impl<
    T: TaskRepository
        + ContractRepository
        + ProjectRepository
        + ProjectMemberRepository
        + UserRepository
        + ApiKeyRepository
        + MetadataFieldRepository
        + AuthenticationPort
        + TaskQueryPort
        + TaskTransitionPort
        + ProjectQueryPort
        + UserQueryPort,
> TaskBackend for T
{
}
