//! Deterministic sample-data seeder for local development and e2e tests.
//!
//! Two modes:
//! - `Reset`: wipes every senko-managed row (preserving the bootstrap
//!   project/user with id=1) and then loads the fixtures.
//! - `Append`: loads the fixtures only when the database has no seeded data.
//!   Re-running on top of an already-seeded DB is a noop.
//!
//! Idempotency is achieved with a `seed` tag on every entity that the seeder
//! creates. `SeederPort::has_seeded_data` looks for this marker.

use std::sync::Arc;

use anyhow::Result;
use clap::ValueEnum;

use crate::application::port::{SeederPort, TaskBackend};

mod fixtures;

/// Mode flag for `senko dev seed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum SeedMode {
    /// Skip if seed data already exists (idempotent re-run).
    #[default]
    Append,
    /// Wipe every senko-managed row then load the fixtures.
    Reset,
}

/// Run the seeder against the given backend.
///
/// `backend` and `seeder` must point to the same underlying database.
/// `bootstrap::create_seeder_backends` is the canonical way to obtain them.
pub async fn run(
    backend: Arc<dyn TaskBackend>,
    seeder: Arc<dyn SeederPort>,
    mode: SeedMode,
) -> Result<()> {
    match mode {
        SeedMode::Reset => {
            seeder.wipe_for_seed().await?;
            fixtures::load(backend.as_ref()).await?;
            eprintln!("dev seed: reset complete");
        }
        SeedMode::Append => {
            if seeder.has_seeded_data().await? {
                eprintln!("dev seed: already seeded (noop)");
                return Ok(());
            }
            fixtures::load(backend.as_ref()).await?;
            eprintln!("dev seed: append complete");
        }
    }
    Ok(())
}
