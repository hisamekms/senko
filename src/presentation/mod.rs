pub mod api;
pub mod cli;
pub mod web;

use std::net::IpAddr;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

const MAX_PORT_RETRIES: u16 = 10;

/// Attempt to bind a TcpListener.
///
/// If `port_is_explicit` is true, fail immediately on bind error.
/// Otherwise, retry with incrementing ports up to `MAX_PORT_RETRIES` times.
/// Returns the bound listener and the actual port used.
pub async fn bind_with_retry(
    bind_ip: IpAddr,
    port: u16,
    port_is_explicit: bool,
) -> Result<(TcpListener, u16)> {
    if port_is_explicit {
        let addr = std::net::SocketAddr::new(bind_ip, port);
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("failed to bind to {addr}"))?;
        return Ok((listener, port));
    }

    for attempt in 0..MAX_PORT_RETRIES {
        let candidate = port
            .checked_add(attempt)
            .context("port number overflow during retry")?;
        let addr = std::net::SocketAddr::new(bind_ip, candidate);
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                if attempt > 0 {
                    tracing::info!(
                        requested_port = port,
                        actual_port = candidate,
                        "default port {port} was unavailable, using port {candidate}"
                    );
                }
                return Ok((listener, candidate));
            }
            Err(e) => {
                tracing::debug!(
                    port = candidate,
                    attempt = attempt + 1,
                    error = %e,
                    "port bind failed, trying next"
                );
            }
        }
    }

    anyhow::bail!(
        "failed to bind to any port in range {}..{} after {MAX_PORT_RETRIES} attempts",
        port,
        port + MAX_PORT_RETRIES - 1
    )
}
