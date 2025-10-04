// Copyright 2025 Oxide Computer Company

//! Common types and protocol definitions for the Central Registry Service (CRS)

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
    /// Generate a deterministic client ID from hostname, OS, and IP address
    /// This ensures the same client gets the same ID across restarts
    pub fn from_client_data(
        hostname: &str,
        os: &str,
        ip_address: &str,
    ) -> Self {
        // Create a deterministic UUID v5 using a namespace and the client data
        let namespace = Uuid::NAMESPACE_DNS;
        let data = format!("{}:{}:{}", hostname, os, ip_address);
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

    /// Optional custom metadata as key-value pairs
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl ClientInfo {
    /// Calculate the deterministic client ID for this client info
    pub fn client_id(&self) -> ClientId {
        ClientId::from_client_data(&self.hostname, &self.os, &self.ip_address)
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
