pub mod autoeq;
pub mod local_scanner;
pub mod lyrics;
pub mod oauth;
pub mod public_sync;
pub mod resolver;
pub mod spotify_web;
pub mod subsonic;
pub mod updater;
pub mod yt_cleaner;
pub mod ytmusic;

use std::sync::OnceLock;
use std::time::Duration;
use thiserror::Error;

static SHARED_HTTP_AGENT: OnceLock<ureq::Agent> = OnceLock::new();
static SHARED_HTTP_AGENT_IPV4: OnceLock<ureq::Agent> = OnceLock::new();

/// Process-wide shared HTTP client (dual-stack) so all provider modules reuse a single rustls
/// root-certificate store and TLS session/connection pool instead of allocating separate agents.
pub fn shared_http_agent() -> &'static ureq::Agent {
    SHARED_HTTP_AGENT.get_or_init(|| {
        let cfg = ureq::Agent::config_builder()
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(Duration::from_secs(8)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .timeout_per_read(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build();
        ureq::Agent::new_with_config(cfg)
    })
}

/// Process-wide shared IPv4-only HTTP client for endpoints (e.g. Spotify Pathfinder, YouTube
/// InnerTube IPv4 fallback) where Windows IPv6 sockets suffer intermittent resets.
pub fn shared_http_agent_ipv4() -> &'static ureq::Agent {
    SHARED_HTTP_AGENT_IPV4.get_or_init(|| {
        let cfg = ureq::Agent::config_builder()
            .ip_family(ureq::config::IpFamily::Ipv4Only)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(Duration::from_secs(8)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .timeout_per_read(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(45)))
            .http_status_as_error(false)
            .build();
        ureq::Agent::new_with_config(cfg)
    })
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network request failed: {0}")]
    Http(String),
    #[error("metadata read failed: {0}")]
    Metadata(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;
