use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::user::{ApiKey, ListSessionsFilter, ListUsersFilter, User, UserId};

#[async_trait]
pub trait UserQueryPort: Send + Sync {
    async fn list_users(&self, filter: &ListUsersFilter) -> Result<ListPage<User>>;
    /// Non-paginated list of raw API keys. Not unified because the endpoint is
    /// not exposed to CLI yet; see task #337 scope. Kept here so the `sessions`
    /// path below can reuse it internally.
    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>>;
    /// Paginated cursor-based list of raw API keys (for sessions / future use).
    /// Ordering: `id ASC`. Expiry filtering is done by the caller.
    async fn list_api_keys_page(
        &self,
        user_id: UserId,
        filter: &ListSessionsFilter,
    ) -> Result<ListPage<ApiKey>>;
}
