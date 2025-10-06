use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oxmon_common::{
    EventType, HostConfig, HostEvent, ServerSession, TimelineBucketState,
};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::net::IpAddr;
use std::str::FromStr;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection
    /// Returns (Database, is_new) where is_new indicates if the database
    /// was newly created
    pub async fn new(db_path: &str) -> Result<(Self, bool)> {
        let db_exists =
            std::path::Path::new(db_path).exists() && db_path != ":memory:";

        let options =
            SqliteConnectOptions::from_str(db_path)?.create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        let db = Self { pool };
        db.run_migrations().await?;

        Ok((db, !db_exists))
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS hosts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hostname TEXT NOT NULL,
                ip_address TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT
                    (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                first_inserted TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS host_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                host_id INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                timestamp TEXT NOT NULL DEFAULT
                    (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                FOREIGN KEY (host_id) REFERENCES hosts(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ping_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                host_id INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                total_count INTEGER NOT NULL,
                avg_latency_ms REAL,
                timestamp TEXT NOT NULL DEFAULT
                    (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                FOREIGN KEY (host_id) REFERENCES hosts(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS server_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL DEFAULT
                    (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                stopped_at TEXT,
                shutdown_type TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update a host
    /// Returns (host_id, is_new) where is_new indicates if the host was
    /// newly created
    pub async fn upsert_host(
        &self,
        config: &HostConfig,
    ) -> Result<(i64, bool)> {
        let ip_str = config.ip_address.to_string();

        // Check if host exists
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM hosts WHERE hostname = ? AND ip_address = ?",
        )
        .bind(&config.hostname)
        .bind(&ip_str)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id,)) = existing {
            Ok((id, false)) // Existing host
        } else {
            let result = sqlx::query(
                "INSERT INTO hosts (hostname, ip_address) VALUES (?, ?)",
            )
            .bind(&config.hostname)
            .bind(&ip_str)
            .execute(&self.pool)
            .await?;

            Ok((result.last_insert_rowid(), true)) // New host
        }
    }

    /// Set first_inserted timestamp for a host
    pub async fn set_first_inserted(
        &self,
        host_id: i64,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE hosts
            SET first_inserted = ?
            WHERE id = ? AND first_inserted IS NULL
            "#,
        )
        .bind(timestamp.to_rfc3339())
        .bind(host_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Record a host event (state transition)
    pub async fn record_event(
        &self,
        host_id: i64,
        event_type: EventType,
    ) -> Result<()> {
        let event_str = match event_type {
            EventType::Online => "online",
            EventType::Offline => "offline",
            EventType::Unknown => "unknown",
        };

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type) VALUES (?, ?)",
        )
        .bind(host_id)
        .bind(event_str)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Record a ping result
    pub async fn record_ping_result(
        &self,
        host_id: i64,
        success_count: u32,
        total_count: u32,
        avg_latency_ms: Option<f64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO ping_results
                (host_id, success_count, total_count, avg_latency_ms)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(host_id)
        .bind(success_count as i64)
        .bind(total_count as i64)
        .bind(avg_latency_ms)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all hosts
    pub async fn get_hosts(&self) -> Result<Vec<(i64, HostConfig)>> {
        let rows = sqlx::query("SELECT id, hostname, ip_address FROM hosts")
            .fetch_all(&self.pool)
            .await?;

        let mut hosts = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let hostname: String = row.get("hostname");
            let ip_str: String = row.get("ip_address");
            let ip_address: IpAddr = ip_str.parse()?;

            hosts.push((
                id,
                HostConfig {
                    hostname,
                    ip_address,
                },
            ));
        }

        Ok(hosts)
    }

    /// Get the last event for a host
    pub async fn get_last_event(
        &self,
        host_id: i64,
    ) -> Result<Option<HostEvent>> {
        let row: Option<(i64, String, String)> = sqlx::query_as(
            r#"
            SELECT id, event_type, timestamp
            FROM host_events
            WHERE host_id = ?
            ORDER BY timestamp DESC
            LIMIT 1
            "#,
        )
        .bind(host_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, event_type_str, timestamp_str)) = row {
            let event_type = match event_type_str.as_str() {
                "online" => EventType::Online,
                "offline" => EventType::Offline,
                "unknown" => EventType::Unknown,
                _ => return Ok(None),
            };

            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?
                .with_timezone(&Utc);

            Ok(Some(HostEvent {
                id,
                host_id,
                event_type,
                timestamp,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get all events for a host
    pub async fn get_host_events(
        &self,
        host_id: i64,
    ) -> Result<Vec<HostEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_type, timestamp
            FROM host_events
            WHERE host_id = ?
            ORDER BY timestamp DESC
            "#,
        )
        .bind(host_id)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let event_type_str: String = row.get("event_type");
            let timestamp_str: String = row.get("timestamp");

            let event_type = match event_type_str.as_str() {
                "online" => EventType::Online,
                "offline" => EventType::Offline,
                "unknown" => EventType::Unknown,
                _ => continue,
            };

            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?
                .with_timezone(&Utc);

            events.push(HostEvent {
                id,
                host_id,
                event_type,
                timestamp,
            });
        }

        Ok(events)
    }

    /// Create a new server session
    pub async fn create_session(&self) -> Result<i64> {
        let result = sqlx::query("INSERT INTO server_sessions DEFAULT VALUES")
            .execute(&self.pool)
            .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get the most recent server session
    pub async fn get_last_session(&self) -> Result<Option<ServerSession>> {
        let row: Option<(i64, String, Option<String>, Option<String>)> =
            sqlx::query_as(
                r#"
            SELECT id, started_at, stopped_at, shutdown_type
            FROM server_sessions
            ORDER BY id DESC
            LIMIT 1
            "#,
            )
            .fetch_optional(&self.pool)
            .await?;

        if let Some((id, started_at_str, stopped_at_str, shutdown_type)) = row {
            let started_at = DateTime::parse_from_rfc3339(&started_at_str)?
                .with_timezone(&Utc);

            let stopped_at = stopped_at_str
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                })
                .transpose()?;

            Ok(Some(ServerSession {
                id,
                started_at,
                stopped_at,
                shutdown_type,
            }))
        } else {
            Ok(None)
        }
    }

    /// Close an open server session
    pub async fn close_session(
        &self,
        session_id: i64,
        stopped_at: DateTime<Utc>,
        shutdown_type: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE server_sessions
            SET stopped_at = ?, shutdown_type = ?
            WHERE id = ?
            "#,
        )
        .bind(stopped_at.to_rfc3339())
        .bind(shutdown_type)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get the timestamp of the last ping result across all hosts
    pub async fn get_last_ping_timestamp(
        &self,
    ) -> Result<Option<DateTime<Utc>>> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT timestamp
            FROM ping_results
            ORDER BY timestamp DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some((timestamp_str,)) = row {
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?
                .with_timezone(&Utc);
            Ok(Some(timestamp))
        } else {
            Ok(None)
        }
    }

    /// Get all server sessions (for testing)
    pub async fn get_all_sessions(&self) -> Result<Vec<ServerSession>> {
        let rows = sqlx::query(
            "SELECT id, started_at, stopped_at, shutdown_type FROM server_sessions ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let started_at_str: String = row.get("started_at");
            let stopped_at_str: Option<String> = row.get("stopped_at");
            let shutdown_type: Option<String> = row.get("shutdown_type");

            let started_at = DateTime::parse_from_rfc3339(&started_at_str)?
                .with_timezone(&Utc);

            let stopped_at = stopped_at_str
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                })
                .transpose()?;

            sessions.push(ServerSession {
                id,
                started_at,
                stopped_at,
                shutdown_type,
            });
        }

        Ok(sessions)
    }

    /// Generate timeline buckets for a host over a time period
    /// Returns a vector of bucket states representing the host's
    /// status over time
    pub async fn get_host_timeline(
        &self,
        host_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        num_buckets: usize,
    ) -> Result<Vec<TimelineBucketState>> {
        let bucket_duration =
            (end_time - start_time).num_seconds() / num_buckets as i64;
        let bucket_duration = Duration::seconds(bucket_duration);

        // Get all events for this host in the time range
        let events = self
            .get_host_events_in_range(host_id, start_time, end_time)
            .await?;

        // Get all server sessions that overlap with the time range
        let sessions = self.get_sessions_in_range(start_time, end_time).await?;

        let mut buckets = Vec::new();

        for i in 0..num_buckets {
            let bucket_start = start_time + bucket_duration * i as i32;
            let bucket_end = bucket_start + bucket_duration;

            let state = self.determine_bucket_state(
                &events,
                &sessions,
                bucket_start,
                bucket_end,
            );

            buckets.push(state);
        }

        Ok(buckets)
    }

    /// Get events for a host within a time range
    async fn get_host_events_in_range(
        &self,
        host_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<HostEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_type, timestamp
            FROM host_events
            WHERE host_id = ? AND timestamp >= ? AND timestamp <= ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(host_id)
        .bind(start_time.to_rfc3339())
        .bind(end_time.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let event_type_str: String = row.get("event_type");
            let timestamp_str: String = row.get("timestamp");

            let event_type = match event_type_str.as_str() {
                "online" => EventType::Online,
                "offline" => EventType::Offline,
                "unknown" => EventType::Unknown,
                _ => continue,
            };

            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?
                .with_timezone(&Utc);

            events.push(HostEvent {
                id,
                host_id,
                event_type,
                timestamp,
            });
        }

        Ok(events)
    }

    /// Get server sessions that overlap with a time range
    async fn get_sessions_in_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<ServerSession>> {
        let rows = sqlx::query(
            r#"
            SELECT id, started_at, stopped_at, shutdown_type
            FROM server_sessions
            WHERE started_at <= ? AND (stopped_at IS NULL OR stopped_at >= ?)
            ORDER BY started_at ASC
            "#,
        )
        .bind(end_time.to_rfc3339())
        .bind(start_time.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for row in rows {
            let id: i64 = row.get("id");
            let started_at_str: String = row.get("started_at");
            let stopped_at_str: Option<String> = row.get("stopped_at");
            let shutdown_type: Option<String> = row.get("shutdown_type");

            let started_at = DateTime::parse_from_rfc3339(&started_at_str)?
                .with_timezone(&Utc);

            let stopped_at = stopped_at_str
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                })
                .transpose()?;

            sessions.push(ServerSession {
                id,
                started_at,
                stopped_at,
                shutdown_type,
            });
        }

        Ok(sessions)
    }

    /// Determine the state of a bucket based on events and sessions
    /// Priority order:
    /// 1. Offline - if ANY offline event occurs during bucket
    /// 2. NoData - if server not running or no valid data
    /// 3. Online - if only online events during bucket
    fn determine_bucket_state(
        &self,
        events: &[HostEvent],
        sessions: &[ServerSession],
        bucket_start: DateTime<Utc>,
        bucket_end: DateTime<Utc>,
    ) -> TimelineBucketState {
        // Check if server was running during this bucket
        let server_running = sessions.iter().any(|session| {
            let session_start = session.started_at;
            let session_end = session.stopped_at.unwrap_or_else(Utc::now);

            // Session overlaps with bucket
            session_start < bucket_end && session_end > bucket_start
        });

        if !server_running {
            return TimelineBucketState::NoData;
        }

        // Get all events that are relevant to this bucket:
        // 1. Events that occur within the bucket
        // 2. The most recent event before the bucket starts
        //    (represents the state at bucket start)
        let events_in_bucket: Vec<_> = events
            .iter()
            .filter(|e| e.timestamp >= bucket_start && e.timestamp < bucket_end)
            .collect();

        // Events are in ascending order (oldest first), so we need
        // the last event before bucket_start
        let event_before_bucket =
            events.iter().rev().find(|e| e.timestamp < bucket_start);

        // Priority 1: If ANY offline event during bucket → Offline
        if events_in_bucket
            .iter()
            .any(|e| e.event_type == EventType::Offline)
        {
            return TimelineBucketState::Offline;
        }

        // Priority 2: Check if we have unknown/no-data state
        // If there's an unknown event in the bucket
        if events_in_bucket
            .iter()
            .any(|e| e.event_type == EventType::Unknown)
        {
            return TimelineBucketState::NoData;
        }

        // If there's an online event in the bucket, host was online
        if events_in_bucket
            .iter()
            .any(|e| e.event_type == EventType::Online)
        {
            return TimelineBucketState::Online;
        }

        // No events in bucket, check state before bucket
        match event_before_bucket {
            Some(event) => match event.event_type {
                EventType::Online => TimelineBucketState::Online,
                EventType::Offline => TimelineBucketState::Offline,
                EventType::Unknown => TimelineBucketState::NoData,
            },
            None => TimelineBucketState::NoData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxmon_common::{EventType, HostConfig};
    use std::net::{IpAddr, Ipv4Addr};

    async fn create_test_db() -> Database {
        let (db, _) = Database::new(":memory:").await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_database_initialization() {
        let db = create_test_db().await;

        // Verify tables exist by querying them
        let result =
            sqlx::query("SELECT * FROM hosts").fetch_all(&db.pool).await;
        assert!(result.is_ok());

        let result = sqlx::query("SELECT * FROM host_events")
            .fetch_all(&db.pool)
            .await;
        assert!(result.is_ok());

        let result = sqlx::query("SELECT * FROM ping_results")
            .fetch_all(&db.pool)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_host_upsert_new() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (id, is_new) = db.upsert_host(&config).await.unwrap();
        assert_eq!(id, 1);
        assert!(is_new, "Should indicate host is new");

        // Verify host was inserted
        let hosts = db.get_hosts().await.unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].1.hostname, "test-host");
    }

    #[tokio::test]
    async fn test_host_upsert_existing() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (id1, is_new1) = db.upsert_host(&config).await.unwrap();
        let (id2, is_new2) = db.upsert_host(&config).await.unwrap();

        // Should return same ID
        assert_eq!(id1, id2);
        assert!(is_new1, "First insert should be new");
        assert!(!is_new2, "Second insert should not be new");

        // Should only have one host
        let hosts = db.get_hosts().await.unwrap();
        assert_eq!(hosts.len(), 1);
    }

    #[tokio::test]
    async fn test_event_recording() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Record online event
        db.record_event(host_id, EventType::Online).await.unwrap();

        // Record offline event
        db.record_event(host_id, EventType::Offline).await.unwrap();

        // Get events
        let events = db.get_host_events(host_id).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::Offline);
        assert_eq!(events[1].event_type, EventType::Online);
    }

    #[tokio::test]
    async fn test_ping_result_storage() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Record ping result
        db.record_ping_result(host_id, 3, 3, Some(15.5))
            .await
            .unwrap();

        // Verify it was stored
        let rows = sqlx::query("SELECT * FROM ping_results WHERE host_id = ?")
            .bind(host_id)
            .fetch_all(&db.pool)
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn test_get_last_event() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Record events
        db.record_event(host_id, EventType::Online).await.unwrap();
        db.record_event(host_id, EventType::Offline).await.unwrap();

        // Get last event
        let last_event = db.get_last_event(host_id).await.unwrap();
        assert!(last_event.is_some());
        assert_eq!(last_event.unwrap().event_type, EventType::Offline);
    }

    #[tokio::test]
    async fn test_get_last_event_none() {
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // No events recorded
        let last_event = db.get_last_event(host_id).await.unwrap();
        assert!(last_event.is_none());
    }

    #[tokio::test]
    async fn test_get_hosts() {
        let db = create_test_db().await;

        let config1 = HostConfig {
            hostname: "host1".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let config2 = HostConfig {
            hostname: "host2".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
        };

        let _ = db.upsert_host(&config1).await.unwrap();
        let _ = db.upsert_host(&config2).await.unwrap();

        let hosts = db.get_hosts().await.unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].1.hostname, "host1");
        assert_eq!(hosts[1].1.hostname, "host2");
    }

    // Timeline bucket state priority tests

    #[tokio::test]
    async fn test_bucket_priority_offline_beats_online() {
        // If a host is offline at any point during a bucket, show offline
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session
        db.create_session().await.unwrap();

        // Bucket from T+0 to T+10 minutes
        let bucket_start = Utc::now();
        let bucket_end = bucket_start + Duration::minutes(10);

        // Events: online at T+2, offline at T+5, online at T+8
        let t2 = bucket_start + Duration::minutes(2);
        let t5 = bucket_start + Duration::minutes(5);
        let t8 = bucket_start + Duration::minutes(8);

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("online")
        .bind(t2.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("offline")
        .bind(t5.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("online")
        .bind(t8.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        // Get session
        let session = db.get_last_session().await.unwrap().unwrap();
        let sessions = vec![session];

        // Get events
        let events = db.get_host_events(host_id).await.unwrap();

        // Determine state - should be Offline (offline event at T+5)
        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::Offline,
            "Bucket should be Offline when any offline event occurs during window"
        );
    }

    #[tokio::test]
    async fn test_bucket_priority_nodata_beats_online() {
        // If server not running during bucket, show NoData even if previous state was online
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session that ends before bucket
        let session_start = Utc::now();
        let session_end = session_start + Duration::minutes(5);

        let session_id = db.create_session().await.unwrap();
        db.close_session(session_id, session_end, "test")
            .await
            .unwrap();

        // Bucket from T+10 to T+20 (after session ended)
        let bucket_start = session_start + Duration::minutes(10);
        let bucket_end = bucket_start + Duration::minutes(10);

        // Event: online at T+2 (during session, before bucket)
        let t2 = session_start + Duration::minutes(2);

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("online")
        .bind(t2.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        // Get session
        let sessions = db.get_all_sessions().await.unwrap();
        let events = db.get_host_events(host_id).await.unwrap();

        // Determine state - should be NoData (server not running during bucket)
        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::NoData,
            "Bucket should be NoData when server not running during window"
        );
    }

    #[tokio::test]
    async fn test_bucket_all_online_shows_online() {
        // If only online events during bucket, show Online
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session
        db.create_session().await.unwrap();

        // Bucket from T+0 to T+10 minutes
        let bucket_start = Utc::now();
        let bucket_end = bucket_start + Duration::minutes(10);

        // Events: online at T+2, online at T+5, online at T+8
        let t2 = bucket_start + Duration::minutes(2);
        let t5 = bucket_start + Duration::minutes(5);
        let t8 = bucket_start + Duration::minutes(8);

        for timestamp in [t2, t5, t8] {
            sqlx::query(
                "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
            )
            .bind(host_id)
            .bind("online")
            .bind(timestamp.to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
        }

        let session = db.get_last_session().await.unwrap().unwrap();
        let sessions = vec![session];
        let events = db.get_host_events(host_id).await.unwrap();

        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::Online,
            "Bucket should be Online when only online events occur during window"
        );
    }

    #[tokio::test]
    async fn test_bucket_inherits_state_from_before() {
        // If no events during bucket, inherit state from before bucket
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session
        db.create_session().await.unwrap();

        let bucket_start = Utc::now();
        let bucket_end = bucket_start + Duration::minutes(10);

        // Event: offline BEFORE bucket starts
        let t_before = bucket_start - Duration::minutes(5);

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("offline")
        .bind(t_before.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        let session = db.get_last_session().await.unwrap().unwrap();
        let sessions = vec![session];
        let events = db.get_host_events(host_id).await.unwrap();

        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::Offline,
            "Bucket should inherit Offline state from event before bucket"
        );
    }

    #[tokio::test]
    async fn test_bucket_unknown_event_shows_nodata() {
        // If unknown event during bucket, show NoData
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session
        db.create_session().await.unwrap();

        let bucket_start = Utc::now();
        let bucket_end = bucket_start + Duration::minutes(10);

        // Events: unknown at T+5 (server restart during bucket)
        let t5 = bucket_start + Duration::minutes(5);

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("unknown")
        .bind(t5.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        let session = db.get_last_session().await.unwrap().unwrap();
        let sessions = vec![session];
        let events = db.get_host_events(host_id).await.unwrap();

        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::NoData,
            "Bucket should be NoData when unknown event occurs during window"
        );
    }

    #[tokio::test]
    async fn test_bucket_offline_beats_unknown() {
        // Offline priority beats unknown/nodata
        let db = create_test_db().await;

        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };
        let (host_id, _) = db.upsert_host(&config).await.unwrap();

        // Create a session
        db.create_session().await.unwrap();

        let bucket_start = Utc::now();
        let bucket_end = bucket_start + Duration::minutes(10);

        // Events: offline at T+2, unknown at T+5
        let t2 = bucket_start + Duration::minutes(2);
        let t5 = bucket_start + Duration::minutes(5);

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("offline")
        .bind(t2.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO host_events (host_id, event_type, timestamp) VALUES (?, ?, ?)",
        )
        .bind(host_id)
        .bind("unknown")
        .bind(t5.to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();

        let session = db.get_last_session().await.unwrap().unwrap();
        let sessions = vec![session];
        let events = db.get_host_events(host_id).await.unwrap();

        let state = db.determine_bucket_state(
            &events,
            &sessions,
            bucket_start,
            bucket_end,
        );

        assert_eq!(
            state,
            oxmon_common::TimelineBucketState::Offline,
            "Bucket should be Offline even with unknown event (offline has priority)"
        );
    }
}
