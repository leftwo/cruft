// Copyright 2025 Oxide Computer Company

//! Common types and protocol definitions for the Central Registry Service (CRS)
//!
//! This crate contains shared data structures and protocol definitions used by
//! both the CRS server and client implementations.
//!
//! # Protocol Overview
//!
//! The CRS protocol is based on REST API with JSON payloads. The protocol
//! supports three main operations:
//!
//! ## Registration
//!
//! Clients send a [`RegisterRequest`] containing their information (hostname,
//! OS, IP address, version, and optional tags). The server responds with a
//! [`RegisterResponse`] containing the client's deterministic ID and the
//! recommended heartbeat interval.
//!
//! ## Heartbeat
//!
//! Clients periodically send [`HeartbeatRequest`] messages to indicate they
//! are still online. The server responds with [`HeartbeatResponse`] containing
//! the current server time.
//!
//! ## Client Listing
//!
//! The server provides a [`ListClientsResponse`] containing all registered
//! clients with their current status and metadata.
//!
//! # Client ID Generation
//!
//! Client IDs are deterministic UUIDs (v5) generated from the client's
//! hostname and operating system. This ensures the same client
//! receives the same ID across restarts.
//!
//! # Client Status
//!
//! Clients are categorized into three states:
//! - [`ClientStatus::Online`] - Recent heartbeat (< 40 seconds, < 1 missed)
//! - [`ClientStatus::Stale`] - Heartbeat 40-80 seconds ago (1-3 missed)
//! - [`ClientStatus::Offline`] - No heartbeat for 80+ seconds (4+ missed)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a client
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub struct ClientId(pub Uuid);

impl ClientId {
    /// Generate a deterministic client ID from hostname, OS, and optional host ID
    /// This ensures the same client gets the same ID across restarts
    pub fn from_client_data(
        hostname: &str,
        os: &str,
        host_id: Option<&str>,
    ) -> Self {
        // Create a deterministic UUID v5 using a namespace and the client data
        let namespace = Uuid::NAMESPACE_DNS;
        let data = if let Some(hid) = host_id {
            format!("{}:{}:{}", hostname, os, hid)
        } else {
            format!("{}:{}", hostname, os)
        };
        Self(Uuid::new_v5(&namespace, data.as_bytes()))
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a client registering with the CRS
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClientInfo {
    /// Hostname of the client machine
    pub hostname: String,

    /// Operating system (e.g., "Linux", "macOS", "Windows")
    pub os: String,

    /// IP address of the client
    pub ip_address: String,

    /// Client software version
    pub version: String,

    /// Host ID of the system (if available)
    #[serde(default)]
    pub host_id: Option<String>,

    /// Optional custom metadata as key-value pairs
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl ClientInfo {
    /// Calculate the deterministic client ID for this client info
    pub fn client_id(&self) -> ClientId {
        ClientId::from_client_data(
            &self.hostname,
            &self.os,
            self.host_id.as_deref(),
        )
    }
}

/// Request to register a new client
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterRequest {
    pub client_info: ClientInfo,
}

/// Response after successful registration
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterResponse {
    /// Assigned/confirmed client ID (deterministic based on client info)
    pub client_id: ClientId,

    /// Recommended heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
}

/// Request to send a heartbeat
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HeartbeatRequest {
    pub client_id: ClientId,
}

/// Response to a heartbeat
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HeartbeatResponse {
    /// Server timestamp when heartbeat was received (RFC3339 format)
    #[schemars(with = "String")]
    pub server_time: DateTime<Utc>,
}

/// Status of a registered client
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ClientStatus {
    /// Client is currently online (recent heartbeat)
    Online,

    /// Client has not sent heartbeat recently but not yet timed out
    Stale,

    /// Client has timed out (no heartbeat for extended period)
    Offline,
}

/// Complete information about a registered client
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisteredClient {
    /// Unique identifier for this client
    pub client_id: ClientId,

    /// Client information
    #[serde(flatten)]
    pub info: ClientInfo,

    /// Current status
    pub status: ClientStatus,

    /// When the client first registered (RFC3339 format)
    #[schemars(with = "String")]
    pub registered_at: DateTime<Utc>,

    /// When the last heartbeat was received (RFC3339 format)
    #[schemars(with = "String")]
    pub last_heartbeat: DateTime<Utc>,
}

/// Response listing all registered clients
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListClientsResponse {
    pub clients: Vec<RegisteredClient>,
    /// Server start time (RFC3339 format)
    #[schemars(with = "String")]
    pub server_start_time: DateTime<Utc>,
}

/// Error types for the CRS protocol
#[derive(Debug, thiserror::Error)]
pub enum CrsError {
    #[error("Client not found: {0}")]
    ClientNotFound(ClientId),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Internal server error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_id_same_inputs_same_uuid() {
        let id1 = ClientId::from_client_data("testhost", "linux", None);
        let id2 = ClientId::from_client_data("testhost", "linux", None);
        assert_eq!(id1, id2, "Same hostname and OS should generate same UUID");
    }

    #[test]
    fn test_client_id_different_hostname() {
        let id1 = ClientId::from_client_data("host1", "linux", None);
        let id2 = ClientId::from_client_data("host2", "linux", None);
        assert_ne!(
            id1, id2,
            "Different hostnames should generate different UUIDs"
        );
    }

    #[test]
    fn test_client_id_different_os() {
        let id1 = ClientId::from_client_data("testhost", "linux", None);
        let id2 = ClientId::from_client_data("testhost", "macos", None);
        assert_ne!(id1, id2, "Different OS should generate different UUIDs");
    }

    #[test]
    fn test_client_id_different_host_id() {
        let id1 =
            ClientId::from_client_data("testhost", "linux", Some("abc123"));
        let id2 =
            ClientId::from_client_data("testhost", "linux", Some("def456"));
        assert_ne!(
            id1, id2,
            "Different host IDs should generate different UUIDs"
        );
    }

    #[test]
    fn test_client_id_with_and_without_host_id() {
        let id1 = ClientId::from_client_data("testhost", "linux", None);
        let id2 =
            ClientId::from_client_data("testhost", "linux", Some("abc123"));
        assert_ne!(
            id1, id2,
            "Same hostname/OS with/without host ID should generate different UUIDs"
        );
    }

    #[test]
    fn test_client_id_is_valid_uuid_v5() {
        let id = ClientId::from_client_data("testhost", "linux", None);
        // UUID v5 has version bits set to 0101 (5)
        assert_eq!(id.0.get_version_num(), 5, "Should be UUID v5");
    }

    #[test]
    fn test_client_info_serialization_roundtrip() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "production".to_string());

        let info = ClientInfo {
            hostname: "testhost".to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: Some("abc123".to_string()),
            tags,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ClientInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.hostname, deserialized.hostname);
        assert_eq!(info.os, deserialized.os);
        assert_eq!(info.ip_address, deserialized.ip_address);
        assert_eq!(info.version, deserialized.version);
        assert_eq!(info.host_id, deserialized.host_id);
        assert_eq!(info.tags, deserialized.tags);
    }

    #[test]
    fn test_client_info_client_id_method() {
        let info = ClientInfo {
            hostname: "testhost".to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: None,
            tags: HashMap::new(),
        };

        let id1 = info.client_id();
        let id2 = ClientId::from_client_data("testhost", "linux", None);
        assert_eq!(id1, id2, "client_id() method should produce correct ID");
    }

    #[test]
    fn test_client_info_client_id_method_with_host_id() {
        let info = ClientInfo {
            hostname: "testhost".to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: Some("abc123".to_string()),
            tags: HashMap::new(),
        };

        let id1 = info.client_id();
        let id2 =
            ClientId::from_client_data("testhost", "linux", Some("abc123"));
        assert_eq!(
            id1, id2,
            "client_id() method should produce correct ID with host_id"
        );
    }

    #[test]
    fn test_client_info_empty_tags() {
        let info = ClientInfo {
            hostname: "testhost".to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: None,
            tags: HashMap::new(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ClientInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.tags.is_empty());
    }

    #[test]
    fn test_client_info_multiple_tags() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "prod".to_string());
        tags.insert("region".to_string(), "us-west".to_string());
        tags.insert("role".to_string(), "worker".to_string());

        let info = ClientInfo {
            hostname: "testhost".to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: None,
            tags: tags.clone(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ClientInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tags.len(), 3);
        assert_eq!(deserialized.tags.get("env"), Some(&"prod".to_string()));
        assert_eq!(
            deserialized.tags.get("region"),
            Some(&"us-west".to_string())
        );
        assert_eq!(deserialized.tags.get("role"), Some(&"worker".to_string()));
    }

    #[test]
    fn test_client_status_serialization() {
        let status = ClientStatus::Online;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"online\"");

        let status = ClientStatus::Stale;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"stale\"");

        let status = ClientStatus::Offline;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"offline\"");
    }

    #[test]
    fn test_client_status_deserialization() {
        let status: ClientStatus = serde_json::from_str("\"online\"").unwrap();
        assert_eq!(status, ClientStatus::Online);

        let status: ClientStatus = serde_json::from_str("\"stale\"").unwrap();
        assert_eq!(status, ClientStatus::Stale);

        let status: ClientStatus = serde_json::from_str("\"offline\"").unwrap();
        assert_eq!(status, ClientStatus::Offline);
    }
}
