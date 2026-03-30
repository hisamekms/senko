use anyhow::Result;
use async_trait::async_trait;

use crate::domain::user::User;

/// Port for API key authentication (lookup user by key hash).
///
/// Separated from `ApiKeyRepository` (which handles CRUD) to keep
/// authentication concerns in the application layer.
#[async_trait]
pub trait AuthenticationPort: Send + Sync {
    /// Whether this backend supports API key authentication (lookup by hash).
    fn supports_api_key_auth(&self) -> bool {
        true
    }

    async fn get_user_by_api_key(&self, key_hash: &str) -> Result<User>;
}
