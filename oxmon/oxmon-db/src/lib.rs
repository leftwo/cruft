use anyhow::Result;
use chrono::{DateTime, Utc};
use oxmon_common::{EventType, HostConfig, HostEvent};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::net::IpAddr;
use std::str::FromStr;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection
    pub async fn new(db_path: &str) -> Result<Self> {
        let options =
            SqliteConnectOptions::from_str(db_path)?.create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        let db = Self { pool };
        db.run_migrations().await?;

        Ok(db)
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
                    (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
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

        Ok(())
    }

    /// Insert or update a host
    pub async fn upsert_host(&self, config: &HostConfig) -> Result<i64> {
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
            Ok(id)
        } else {
            let result = sqlx::query(
                "INSERT INTO hosts (hostname, ip_address) VALUES (?, ?)",
            )
            .bind(&config.hostname)
            .bind(&ip_str)
            .execute(&self.pool)
            .await?;

            Ok(result.last_insert_rowid())
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxmon_common::{EventType, HostConfig};
    use std::net::{IpAddr, Ipv4Addr};

    async fn create_test_db() -> Database {
        Database::new(":memory:").await.unwrap()
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

        let id = db.upsert_host(&config).await.unwrap();
        assert_eq!(id, 1);

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

        let id1 = db.upsert_host(&config).await.unwrap();
        let id2 = db.upsert_host(&config).await.unwrap();

        // Should return same ID
        assert_eq!(id1, id2);

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

        let host_id = db.upsert_host(&config).await.unwrap();

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

        let host_id = db.upsert_host(&config).await.unwrap();

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

        let host_id = db.upsert_host(&config).await.unwrap();

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

        let host_id = db.upsert_host(&config).await.unwrap();

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

        db.upsert_host(&config1).await.unwrap();
        db.upsert_host(&config2).await.unwrap();

        let hosts = db.get_hosts().await.unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].1.hostname, "host1");
        assert_eq!(hosts[1].1.hostname, "host2");
    }
}
