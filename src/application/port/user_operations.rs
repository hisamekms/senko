use anyhow::Result;
use async_trait::async_trait;

use crate::domain::user::{ApiKey, ApiKeyWithSecret, CreateUserParams, UpdateUserParams, User, UserId};
use crate::infra::config::SessionConfig;

/// Application-level port that exposes all user operations.
///
/// Both local (`UserService`) and remote implementations can satisfy this trait,
/// allowing the presentation layer to depend only on the abstraction rather than
/// a concrete service type.
#[async_trait]
pub trait UserOperations: Send + Sync {
    // --- User management ---

    async fn list_users(&self) -> Result<Vec<User>>;
    async fn create_user(&self, params: &CreateUserParams) -> Result<User>;
    async fn get_user(&self, id: UserId) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn get_user_by_sub(&self, sub: &str) -> Result<User>;
    async fn update_user(&self, id: UserId, params: &UpdateUserParams) -> Result<User>;
    async fn delete_user(&self, id: UserId) -> Result<()>;

    // --- API Key management ---

    async fn create_api_key(
        &self,
        user_id: UserId,
        name: &str,
        device_name: Option<&str>,
    ) -> Result<ApiKeyWithSecret>;
    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_id: i64, user_id: UserId) -> Result<()>;

    // --- Session management ---

    async fn get_or_create_user(
        &self,
        sub: &str,
        username: &str,
        display_name: Option<&str>,
        email: Option<&str>,
    ) -> Result<User>;
    async fn create_session_token(
        &self,
        user_id: UserId,
        device_name: Option<&str>,
        session_config: &SessionConfig,
    ) -> Result<ApiKeyWithSecret>;
    async fn list_active_sessions(
        &self,
        user_id: UserId,
        session_config: &SessionConfig,
    ) -> Result<Vec<ApiKey>>;
    async fn revoke_session(&self, key_id: i64, user_id: UserId) -> Result<()>;
    async fn revoke_all_sessions(&self, user_id: UserId) -> Result<()>;

    // --- Proxy passthrough ---

    /// Fetch `/auth/me` from the upstream server. Only implemented for remote/proxy adapters;
    /// local implementations return an error.
    async fn fetch_me(&self) -> Result<serde_json::Value>;
}
