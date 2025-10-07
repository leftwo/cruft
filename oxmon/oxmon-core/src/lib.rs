use anyhow::Result;
use chrono::Utc;
use oxmon_common::{HostConfig, PingResult};
use std::net::IpAddr;
use std::time::Duration;
use surge_ping::{Client, Config, PingIdentifier, PingSequence};

mod config;
mod monitor;

pub use config::load_hosts_from_file;
pub use monitor::Monitor;

/// Ping a host once with 10 second timeout
pub async fn ping_host(host: &HostConfig) -> Result<PingResult> {
    let config = Config::default();
    let client = Client::new(&config)?;

    let mut latencies = Vec::new();
    let total_count = 1;
    let timeout = Duration::from_secs(10);
    let timestamp = Utc::now();

    let payload = [0; 56];

    match host.ip_address {
        IpAddr::V4(addr) => {
            let mut pinger =
                client.pinger(addr.into(), PingIdentifier(1)).await;

            if let Ok(Ok((_, duration))) = tokio::time::timeout(
                timeout,
                pinger.ping(PingSequence(0), &payload),
            )
            .await
            {
                latencies.push(duration.as_secs_f64() * 1000.0);
            }
        }
        IpAddr::V6(addr) => {
            let mut pinger =
                client.pinger(addr.into(), PingIdentifier(1)).await;

            if let Ok(Ok((_, duration))) = tokio::time::timeout(
                timeout,
                pinger.ping(PingSequence(0), &payload),
            )
            .await
            {
                latencies.push(duration.as_secs_f64() * 1000.0);
            }
        }
    }

    let success_count = latencies.len() as u32;

    Ok(PingResult {
        hostname: host.hostname.clone(),
        ip_address: host.ip_address,
        success_count,
        total_count,
        latencies_ms: latencies,
        timestamp,
    })
}
