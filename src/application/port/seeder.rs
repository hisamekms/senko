//! Port for the developer-only seeder.
//!
//! Gated behind the `dev-tools` feature so the public release binary cannot
//! invoke schema-wide deletions. Implemented directly on the concrete backend
//! types (SqliteBackend, PostgresBackend) using raw SQL.

use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait SeederPort: Send + Sync {
    /// Delete every senko-managed row except the bootstrap rows that
    /// migrations create (project id=1, user id=1, and their owner
    /// project_member). Schema is preserved.
    async fn wipe_for_seed(&self) -> Result<()>;

    /// Return true if any task carries the seed marker tag. Used by the
    /// `append` mode to make a re-run a noop.
    async fn has_seeded_data(&self) -> Result<bool>;
}
