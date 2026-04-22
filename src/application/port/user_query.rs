use anyhow::Result;
use async_trait::async_trait;

use crate::domain::user::{ApiKey, User, UserId};

#[async_trait]
pub trait UserQueryPort: Send + Sync {
    async fn list_users(&self) -> Result<Vec<User>>;
    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>>;
}
