use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use oxmon_common::{EventType, HostConfig, HostStatus, HostTimeline, Status};
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
    session_id: i64,
}

impl Monitor {
    /// Create a new monitor
    /// If hosts is empty, loads existing hosts from database
    /// Handles server restart by recording gaps in monitoring
    pub async fn new(
        db: Arc<Database>,
        hosts: Vec<HostConfig>,
    ) -> Result<Self> {
        // Handle previous session if it exists
        if let Some(last_session) = db.get_last_session().await? {
            // If last session has no stopped_at, server crashed
            if last_session.stopped_at.is_none() {
                // Find the last ping timestamp to estimate when server stopped
                if let Some(last_ping) = db.get_last_ping_timestamp().await? {
                    db.close_session(last_session.id, last_ping, "crashed")
                        .await?;
                } else {
                    // No pings recorded, use session start time
                    db.close_session(
                        last_session.id,
                        last_session.started_at,
                        "unknown",
                    )
                    .await?;
                }
            }
        }

        // Create new session for this server run
        let session_id = db.create_session().await?;

        let host_ids = if hosts.is_empty() {
            // Load hosts from database - these are existing hosts
            let existing_hosts = db.get_hosts().await?;

            // Record "unknown" event for existing hosts (server was down)
            for (host_id, _) in &existing_hosts {
                db.record_event(*host_id, EventType::Unknown).await?;
            }

            existing_hosts
        } else {
            // Upsert all hosts into the database
            let mut host_ids = Vec::new();
            for host in &hosts {
                let (id, is_new) = db.upsert_host(host).await?;

                // Record appropriate initial event:
                // - New hosts: "offline" (starting to monitor)
                // - Existing hosts: "unknown" (server was down, state unknown)
                if is_new {
                    db.record_event(id, EventType::Offline).await?;
                } else {
                    db.record_event(id, EventType::Unknown).await?;
                }

                host_ids.push((id, host.clone()));
            }
            host_ids
        };

        // Initialize status_map with all hosts (status unknown until first ping)
        let mut initial_status_map = HashMap::new();
        for (host_id, host_config) in &host_ids {
            let initial_status = HostStatus {
                id: *host_id,
                hostname: host_config.hostname.clone(),
                ip_address: host_config.ip_address,
                status: Status::Offline, // Default to offline until first ping
                last_check: Utc::now(),
                success_count: 0,
                total_count: 0,
                avg_latency_ms: None,
            };
            initial_status_map.insert(*host_id, initial_status);
        }

        Ok(Self {
            db,
            hosts: host_ids,
            status_map: Arc::new(RwLock::new(initial_status_map)),
            session_id,
        })
    }

    /// Get current status of all hosts
    pub async fn get_status(&self) -> Vec<HostStatus> {
        let map = self.status_map.read().await;
        map.values().cloned().collect()
    }

    /// Shutdown the monitor gracefully, closing the current session
    pub async fn shutdown(&self) -> Result<()> {
        let stopped_at = Utc::now();
        self.db
            .close_session(self.session_id, stopped_at, "graceful")
            .await?;
        Ok(())
    }

    /// Get timeline for all hosts over a time period
    pub async fn get_timelines(
        &self,
        duration_hours: u32,
        num_buckets: usize,
    ) -> Result<Vec<HostTimeline>> {
        let end_time = Utc::now();
        let start_time =
            end_time - ChronoDuration::hours(duration_hours as i64);
        let bucket_duration_secs = (duration_hours * 3600) / num_buckets as u32;

        let map = self.status_map.read().await;
        let mut timelines = Vec::new();

        for (host_id, config) in &self.hosts {
            let buckets = self
                .db
                .get_host_timeline(*host_id, start_time, end_time, num_buckets)
                .await?;

            let current_status = map
                .get(host_id)
                .map(|s| s.status)
                .unwrap_or(Status::Offline);

            timelines.push(HostTimeline {
                id: *host_id,
                hostname: config.hostname.clone(),
                ip_address: config.ip_address,
                current_status,
                buckets,
                bucket_duration_secs,
                start_time,
                end_time,
            });
        }

        Ok(timelines)
    }

    /// Start monitoring loop (pings every 10 seconds)
    pub async fn start(self: Arc<Self>) -> Result<()> {
        // Do an immediate first check on startup
        self.check_all_hosts().await?;

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

        // Set first_connected timestamp if host came online
        if new_status == Status::Online {
            db.set_first_connected(host_id, result.timestamp).await?;
        }

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
    async fn test_hosts_visible_immediately_on_startup() {
        // Create an in-memory database
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        // Create test hosts
        let host1 = HostConfig {
            hostname: "test-host-1".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let host2 = HostConfig {
            hostname: "test-host-2".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
        };

        // Create monitor with hosts
        let monitor =
            Monitor::new(db.clone(), vec![host1.clone(), host2.clone()])
                .await
                .unwrap();

        // IMMEDIATELY get status without waiting for any pings
        let status = monitor.get_status().await;

        // Should have 2 hosts visible right away
        assert_eq!(status.len(), 2);

        // Verify host details are present
        let hostnames: Vec<_> =
            status.iter().map(|h| h.hostname.as_str()).collect();
        assert!(hostnames.contains(&"test-host-1"));
        assert!(hostnames.contains(&"test-host-2"));

        // All hosts should have initial status (offline until first ping)
        for host in &status {
            assert_eq!(host.status, Status::Offline);
            assert_eq!(host.success_count, 0);
            assert_eq!(host.total_count, 0);
            assert_eq!(host.avg_latency_ms, None);
        }
    }

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

    #[tokio::test]
    async fn test_monitor_updates_status_after_ping() {
        // Create an in-memory database
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        // Use localhost which should respond to pings
        let host = HostConfig {
            hostname: "localhost".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        };

        // Create monitor
        let monitor =
            Arc::new(Monitor::new(db.clone(), vec![host]).await.unwrap());

        // Check initial status - should be offline
        let initial_status = monitor.get_status().await;
        assert_eq!(initial_status.len(), 1);
        assert_eq!(initial_status[0].status, Status::Offline);
        assert_eq!(initial_status[0].total_count, 0);

        // Manually trigger one check cycle (simulating what the monitoring loop does)
        monitor.check_all_hosts().await.unwrap();

        // Now status should be updated
        let updated_status = monitor.get_status().await;
        assert_eq!(updated_status.len(), 1);

        // Localhost should be online after ping
        assert_eq!(updated_status[0].status, Status::Online);
        assert_eq!(updated_status[0].total_count, 3); // 3 pings attempted
        assert!(updated_status[0].success_count > 0); // At least some succeeded
    }

    #[tokio::test]
    async fn test_server_restart_records_gap() {
        use oxmon_common::EventType;

        // Create a database file for this test
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        // Create a test host
        let host = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        // First server session: Start server
        let monitor1 =
            Monitor::new(db.clone(), vec![host.clone()]).await.unwrap();
        let host_id = monitor1.hosts[0].0;

        // Verify first session was created
        let session1 = db.get_last_session().await.unwrap().unwrap();
        assert!(session1.stopped_at.is_none()); // Still running
        assert_eq!(session1.id, 1);

        // Verify "offline" event was recorded for new host
        let events = db.get_host_events(host_id).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Offline);

        // Simulate some activity (ping result)
        db.record_ping_result(host_id, 3, 3, Some(15.0))
            .await
            .unwrap();
        db.record_event(host_id, EventType::Online).await.unwrap();

        // Drop monitor1 to simulate server crash (no clean shutdown)
        drop(monitor1);

        // Second server session: Restart server
        let monitor2 = Monitor::new(db.clone(), Vec::new()).await.unwrap();
        assert_eq!(monitor2.hosts.len(), 1);

        // Verify old session was closed with "crashed" status
        let sessions = db.get_last_session().await.unwrap().unwrap();
        assert_eq!(sessions.id, 2); // New session

        // Check that session 1 was closed
        let all_sessions = db.get_all_sessions().await.unwrap();

        assert_eq!(all_sessions.len(), 2);
        assert_eq!(all_sessions[0].id, 1);
        assert_eq!(all_sessions[0].shutdown_type, Some("crashed".to_string()));
        assert!(all_sessions[0].stopped_at.is_some());
        assert_eq!(all_sessions[1].id, 2);
        assert_eq!(all_sessions[1].shutdown_type, None); // Current session, still running
        assert!(all_sessions[1].stopped_at.is_none());

        // Verify "unknown" event was recorded again after restart
        let events = db.get_host_events(host_id).await.unwrap();
        // Should have: offline (session 1 start, new host), online (activity), unknown (session 2 start)
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].event_type, EventType::Offline); // oldest - new host
        assert_eq!(events[1].event_type, EventType::Online);
        assert_eq!(events[0].event_type, EventType::Unknown); // newest - restart
    }

    #[tokio::test]
    async fn test_new_host_gets_offline_event() {
        // New hosts should get an "offline" event, not "unknown"
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        let host = HostConfig {
            hostname: "new-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
        };

        // First time seeing this host
        let monitor = Monitor::new(db.clone(), vec![host]).await.unwrap();
        let host_id = monitor.hosts[0].0;

        // Should have exactly one event: offline
        let events = db.get_host_events(host_id).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            EventType::Offline,
            "New host should start with offline event"
        );
    }

    #[tokio::test]
    async fn test_existing_host_gets_unknown_event_on_restart() {
        // Existing hosts should get "unknown" event on server restart
        let (db, _) = Database::new(":memory:").await.unwrap();
        let db = Arc::new(db);

        let host = HostConfig {
            hostname: "existing-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101)),
        };

        // First session - new host
        let monitor1 =
            Monitor::new(db.clone(), vec![host.clone()]).await.unwrap();
        let host_id = monitor1.hosts[0].0;

        // Simulate some activity
        db.record_event(host_id, EventType::Online).await.unwrap();

        drop(monitor1);

        // Second session - existing host
        let _monitor2 = Monitor::new(db.clone(), vec![host]).await.unwrap();

        // Should have: offline (initial), online (activity), unknown (restart)
        let events = db.get_host_events(host_id).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].event_type, EventType::Offline); // initial
        assert_eq!(events[1].event_type, EventType::Online); // activity
        assert_eq!(
            events[0].event_type,
            EventType::Unknown,
            "Existing host should get unknown event on restart"
        );
    }
}
