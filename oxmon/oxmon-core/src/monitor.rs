use anyhow::Result;
use oxmon_common::{HostConfig, HostStatus, Status};
use oxmon_db::Database;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;

use crate::ping_host;

pub struct Monitor {
    db: Arc<Database>,
    hosts: Vec<(i64, HostConfig)>,
    status_map: Arc<RwLock<HashMap<i64, HostStatus>>>,
}

impl Monitor {
    /// Create a new monitor
    /// If hosts is empty, loads existing hosts from database
    pub async fn new(
        db: Arc<Database>,
        hosts: Vec<HostConfig>,
    ) -> Result<Self> {
        let host_ids = if hosts.is_empty() {
            // Load hosts from database
            db.get_hosts().await?
        } else {
            // Upsert all hosts into the database
            let mut host_ids = Vec::new();
            for host in &hosts {
                let id = db.upsert_host(host).await?;
                host_ids.push((id, host.clone()));
            }
            host_ids
        };

        Ok(Self {
            db,
            hosts: host_ids,
            status_map: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get current status of all hosts
    pub async fn get_status(&self) -> Vec<HostStatus> {
        let map = self.status_map.read().await;
        map.values().cloned().collect()
    }

    /// Start monitoring loop (pings every 10 seconds)
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let mut interval = time::interval(Duration::from_secs(10));

        loop {
            interval.tick().await;
            self.check_all_hosts().await?;
        }
    }

    /// Check all hosts
    async fn check_all_hosts(&self) -> Result<()> {
        let mut tasks = Vec::new();

        for (host_id, host) in &self.hosts {
            let host_id = *host_id;
            let host = host.clone();
            let db = self.db.clone();
            let status_map = self.status_map.clone();

            tasks.push(tokio::spawn(async move {
                Self::check_host(host_id, host, db, status_map).await
            }));
        }

        // Wait for all tasks to complete
        for task in tasks {
            let _ = task.await;
        }

        Ok(())
    }

    /// Check a single host and update status
    async fn check_host(
        host_id: i64,
        host: HostConfig,
        db: Arc<Database>,
        status_map: Arc<RwLock<HashMap<i64, HostStatus>>>,
    ) -> Result<()> {
        let result = ping_host(&host).await?;
        let new_status = if result.is_online() {
            Status::Online
        } else {
            Status::Offline
        };

        // Get previous status
        let prev_status = {
            let map = status_map.read().await;
            map.get(&host_id).map(|s| s.status)
        };

        // Record event if status changed
        if prev_status.is_none() || prev_status != Some(new_status) {
            db.record_event(host_id, new_status.into()).await?;
        }

        // Record ping result
        db.record_ping_result(
            host_id,
            result.success_count,
            result.total_count,
            result.avg_latency_ms(),
        )
        .await?;

        // Update status map
        let host_status = HostStatus {
            id: host_id,
            hostname: host.hostname.clone(),
            ip_address: host.ip_address,
            status: new_status,
            last_check: result.timestamp,
            success_count: result.success_count,
            total_count: result.total_count,
            avg_latency_ms: result.avg_latency_ms(),
        };

        let mut map = status_map.write().await;
        map.insert(host_id, host_status);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxmon_common::HostConfig;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_monitor_loads_hosts_from_database() {
        // Create an in-memory database
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        // Create some test hosts
        let host1 = HostConfig {
            hostname: "test-host-1".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let host2 = HostConfig {
            hostname: "test-host-2".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
        };

        // First run: Create monitor with hosts from file
        let hosts = vec![host1.clone(), host2.clone()];
        let monitor1 = Monitor::new(db.clone(), hosts).await.unwrap();
        assert_eq!(monitor1.hosts.len(), 2);
        assert_eq!(monitor1.hosts[0].1.hostname, "test-host-1");
        assert_eq!(monitor1.hosts[1].1.hostname, "test-host-2");

        // Second run: Create monitor with no hosts (load from database)
        let monitor2 = Monitor::new(db.clone(), Vec::new()).await.unwrap();
        assert_eq!(monitor2.hosts.len(), 2);
        assert_eq!(monitor2.hosts[0].1.hostname, "test-host-1");
        assert_eq!(monitor2.hosts[1].1.hostname, "test-host-2");

        // Verify host IDs match between runs
        assert_eq!(monitor1.hosts[0].0, monitor2.hosts[0].0);
        assert_eq!(monitor1.hosts[1].0, monitor2.hosts[1].0);
    }
}
