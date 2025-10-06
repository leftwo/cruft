use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Server session tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSession {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub shutdown_type: Option<String>, // "clean", "crashed", "unknown"
}

/// Configuration for a single host to monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    pub hostname: String,
    pub ip_address: IpAddr,
}

/// Current status of a monitored host
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HostStatus {
    pub id: i64,
    pub hostname: String,
    pub ip_address: IpAddr,
    pub status: Status,
    pub last_check: DateTime<Utc>,
    pub success_count: u32,
    pub total_count: u32,
    pub avg_latency_ms: Option<f64>,
}

/// Host online/offline status
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Online,
    Offline,
}

/// Result of a ping check (3 pings)
#[derive(Debug, Clone)]
pub struct PingResult {
    pub hostname: String,
    pub ip_address: IpAddr,
    pub success_count: u32,
    pub total_count: u32,
    pub latencies_ms: Vec<f64>,
    pub timestamp: DateTime<Utc>,
}

impl PingResult {
    pub fn avg_latency_ms(&self) -> Option<f64> {
        if self.latencies_ms.is_empty() {
            None
        } else {
            Some(
                self.latencies_ms.iter().sum::<f64>()
                    / self.latencies_ms.len() as f64,
            )
        }
    }

    pub fn is_online(&self) -> bool {
        self.success_count > 0
    }
}

/// State transition event for a host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEvent {
    pub id: i64,
    pub host_id: i64,
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
}

/// Type of state transition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Online,
    Offline,
    Unknown, // Server restarted, host state unknown during downtime
}

impl From<Status> for EventType {
    fn from(status: Status) -> Self {
        match status {
            Status::Online => EventType::Online,
            Status::Offline => EventType::Offline,
        }
    }
}

/// Timeline bucket state for visualization
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum TimelineBucketState {
    Online,
    Offline,
    NoData, // Monitoring gap (server was down)
}

/// Timeline representation for a host
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HostTimeline {
    pub id: i64,
    pub hostname: String,
    pub ip_address: IpAddr,
    pub current_status: Status,
    pub buckets: Vec<TimelineBucketState>,
    pub bucket_duration_secs: u32,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_host_config_serialization() {
        let config = HostConfig {
            hostname: "test-host".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: HostConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.hostname, deserialized.hostname);
        assert_eq!(config.ip_address, deserialized.ip_address);
    }

    #[test]
    fn test_status_serialization() {
        let online = Status::Online;
        let json = serde_json::to_string(&online).unwrap();
        assert_eq!(json, r#""online""#);

        let offline = Status::Offline;
        let json = serde_json::to_string(&offline).unwrap();
        assert_eq!(json, r#""offline""#);
    }

    #[test]
    fn test_ping_result_avg_latency_empty() {
        let result = PingResult {
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            success_count: 0,
            total_count: 3,
            latencies_ms: vec![],
            timestamp: Utc::now(),
        };

        assert_eq!(result.avg_latency_ms(), None);
    }

    #[test]
    fn test_ping_result_avg_latency_single() {
        let result = PingResult {
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            success_count: 1,
            total_count: 3,
            latencies_ms: vec![50.0],
            timestamp: Utc::now(),
        };

        assert_eq!(result.avg_latency_ms(), Some(50.0));
    }

    #[test]
    fn test_ping_result_avg_latency_multiple() {
        let result = PingResult {
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            success_count: 3,
            total_count: 3,
            latencies_ms: vec![10.0, 20.0, 30.0],
            timestamp: Utc::now(),
        };

        assert_eq!(result.avg_latency_ms(), Some(20.0));
    }

    #[test]
    fn test_ping_result_is_online_success() {
        let result = PingResult {
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            success_count: 1,
            total_count: 3,
            latencies_ms: vec![50.0],
            timestamp: Utc::now(),
        };

        assert!(result.is_online());
    }

    #[test]
    fn test_ping_result_is_online_failure() {
        let result = PingResult {
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            success_count: 0,
            total_count: 3,
            latencies_ms: vec![],
            timestamp: Utc::now(),
        };

        assert!(!result.is_online());
    }

    #[test]
    fn test_event_type_from_status() {
        assert_eq!(EventType::from(Status::Online), EventType::Online);
        assert_eq!(EventType::from(Status::Offline), EventType::Offline);
    }

    #[test]
    fn test_host_status_serialization() {
        let status = HostStatus {
            id: 1,
            hostname: "test".to_string(),
            ip_address: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            status: Status::Online,
            last_check: Utc::now(),
            success_count: 3,
            total_count: 3,
            avg_latency_ms: Some(15.5),
        };

        let json = serde_json::to_string(&status).unwrap();
        let deserialized: HostStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(status.id, deserialized.id);
        assert_eq!(status.hostname, deserialized.hostname);
        assert_eq!(status.status, deserialized.status);
    }
}
