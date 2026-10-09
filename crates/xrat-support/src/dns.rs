use async_trait::async_trait;
use std::net::SocketAddr;
mod bootstrap;
pub use bootstrap::resolve_udp;

#[allow(
    clippy::double_must_use,
    reason = "async_trait adds must_use to methods returning already must-use boxed futures"
)]
#[async_trait]
pub trait DnsResolver: Send + Sync {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>>;
}
pub struct TokioDnsResolver;
#[async_trait]
impl DnsResolver for TokioDnsResolver {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.collect())
    }
}
