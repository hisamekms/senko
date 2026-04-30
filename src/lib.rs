pub mod application;
// bootstrap: composition root — wires all layers together, intentionally at crate root
pub mod bootstrap;
#[cfg(feature = "dev-tools")]
pub mod dev;
pub mod domain;
pub mod infra;
pub mod presentation;
