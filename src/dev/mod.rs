//! Developer-only utilities, gated behind the `dev-tools` cargo feature.
//!
//! Nothing in this module is part of the public senko binary. It exists so
//! local development environments and the e2e harness can populate the
//! database with rich, deterministic sample data without going through the
//! public CLI surface (which intentionally has no seeding command).

pub mod seeder;
