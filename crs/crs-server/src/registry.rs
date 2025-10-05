// Copyright 2025 Oxide Computer Company

//! Client registry implementation
//!
//! This module provides the [`Registry`] type which manages all registered
//! clients and their status. The registry is thread-safe and can be shared
//! across multiple async tasks.

use chrono::{Duration, Utc};
use crs_common::{ClientId, ClientInfo, ClientStatus, RegisteredClient};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Thresholds for client status transitions (in seconds)
/// Heartbeat interval is 10 seconds
/// Offline: 1.5x heartbeat interval (15 seconds)
const HEARTBEAT_INTERVAL_SECS: i64 = 10;
const OFFLINE_THRESHOLD_SECS: i64 = (HEARTBEAT_INTERVAL_SECS * 3) / 2;

/// Registry for tracking connected clients
///
/// The registry maintains an in-memory map of all registered clients and
/// their current status. It is thread-safe and can be cloned cheaply (uses
/// `Arc` internally).
///
/// # Example
///
/// ```no_run
/// # use crs_server::registry::Registry;
/// # use crs_common::ClientInfo;
/// let registry = Registry::new();
/// // Register a client (would normally come from API request)
/// # let client_info = ClientInfo {
/// #     hostname: "example".to_string(),
/// #     os: "Linux".to_string(),
/// #     ip_address: "192.168.1.100".to_string(),
/// #     version: "1.0.0".to_string(),
/// #     host_id: None,
/// #     tags: Default::default(),
/// # };
/// let client_id = registry.register(client_info);
/// ```
#[derive(Clone)]
pub struct Registry {
    clients: Arc<RwLock<HashMap<ClientId, RegisteredClient>>>,
}

impl Registry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new client or update existing client
    ///
    /// If the client is already registered (based on deterministic client ID),
    /// this updates the client information but preserves the original
    /// first_connected timestamp. The client is marked as online and the
    /// last heartbeat time is updated to now.
    pub fn register(&self, info: ClientInfo) -> ClientId {
        let client_id = info.client_id();
        let now = Utc::now();

        let mut clients = self.clients.write().unwrap();

        let (first_connected, registered_at) = if let Some(existing) =
            clients.get(&client_id)
        {
            // Preserve first_connected, update registered_at to now for reconnection
            (existing.first_connected, now)
        } else {
            // New client - both timestamps are now
            (now, now)
        };

        let registered_client = RegisteredClient {
            client_id,
            info,
            status: ClientStatus::Online,
            first_connected,
            registered_at,
            last_heartbeat: now,
        };

        clients.insert(client_id, registered_client);
        client_id
    }

    /// Record a heartbeat from a client
    ///
    /// Updates the last heartbeat timestamp and marks the client as online.
    /// Returns an error if the client is not registered.
    pub fn heartbeat(&self, client_id: ClientId) -> Result<(), RegistryError> {
        let mut clients = self.clients.write().unwrap();

        let client = clients
            .get_mut(&client_id)
            .ok_or(RegistryError::ClientNotFound(client_id))?;

        client.last_heartbeat = Utc::now();
        client.status = ClientStatus::Online;

        Ok(())
    }

    /// Get all registered clients
    pub fn list_clients(&self) -> Vec<RegisteredClient> {
        let clients = self.clients.read().unwrap();
        clients.values().cloned().collect()
    }

    /// Update client statuses based on last heartbeat time
    ///
    /// Iterates through all registered clients and updates their status
    /// based on how long ago their last heartbeat was:
    /// - Online: last heartbeat < 15 seconds ago (< 1.5x heartbeat interval)
    /// - Offline: last heartbeat >= 15 seconds ago (>= 1.5x heartbeat interval)
    ///
    /// This is called periodically by a background task.
    pub fn update_statuses(&self) {
        let now = Utc::now();
        let mut clients = self.clients.write().unwrap();

        for client in clients.values_mut() {
            let elapsed = now - client.last_heartbeat;

            client.status = if elapsed
                < Duration::try_seconds(OFFLINE_THRESHOLD_SECS).unwrap()
            {
                ClientStatus::Online
            } else {
                ClientStatus::Offline
            };
        }
    }

    /// Set a client's last heartbeat time (for testing)
    #[cfg(any(test, feature = "test-utils"))]
    pub fn set_last_heartbeat(
        &self,
        client_id: ClientId,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) {
        let mut clients = self.clients.write().unwrap();
        if let Some(client) = clients.get_mut(&client_id) {
            client.last_heartbeat = timestamp;
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry errors
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Client not found: {0}")]
    ClientNotFound(ClientId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_client_info(hostname: &str) -> ClientInfo {
        ClientInfo {
            hostname: hostname.to_string(),
            os: "linux".to_string(),
            ip_address: "192.168.1.100".to_string(),
            version: "1.0.0".to_string(),
            host_id: None,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn test_registry_new_client_registration() {
        let registry = Registry::new();
        let info = create_test_client_info("testhost");

        let client_id = registry.register(info.clone());

        // Verify client is in registry
        let clients = registry.list_clients();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].client_id, client_id);
        assert_eq!(clients[0].info.hostname, "testhost");
        assert_eq!(clients[0].status, ClientStatus::Online);
    }

    #[test]
    fn test_registry_re_registration_preserves_first_connected() {
        let registry = Registry::new();
        let info = create_test_client_info("testhost");

        // First registration
        let client_id = registry.register(info.clone());
        let clients = registry.list_clients();
        let first_connected = clients[0].first_connected;

        // Wait a bit then re-register
        std::thread::sleep(std::time::Duration::from_millis(10));
        registry.register(info.clone());

        // Verify first_connected timestamp is preserved, but registered_at updates
        let clients = registry.list_clients();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].client_id, client_id);
        assert_eq!(clients[0].first_connected, first_connected);
        assert!(clients[0].registered_at > first_connected);
    }

    #[test]
    fn test_registry_heartbeat_updates_timestamp() {
        let registry = Registry::new();
        let info = create_test_client_info("testhost");

        let client_id = registry.register(info);
        let clients = registry.list_clients();
        let first_heartbeat = clients[0].last_heartbeat;

        // Wait a bit then send heartbeat
        std::thread::sleep(std::time::Duration::from_millis(10));
        registry.heartbeat(client_id).unwrap();

        // Verify heartbeat timestamp updated
        let clients = registry.list_clients();
        assert!(clients[0].last_heartbeat > first_heartbeat);
        assert_eq!(clients[0].status, ClientStatus::Online);
    }

    #[test]
    fn test_registry_heartbeat_unknown_client() {
        let registry = Registry::new();
        let unknown_id = ClientId::from_client_data("unknown", "linux", None);

        let result = registry.heartbeat(unknown_id);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RegistryError::ClientNotFound(_)
        ));
    }

    #[test]
    fn test_registry_offline_threshold() {
        let registry = Registry::new();
        let info = create_test_client_info("testhost");

        let client_id = registry.register(info);

        // Set to 10 seconds ago (should be Online - < 15s)
        {
            let mut clients = registry.clients.write().unwrap();
            let client = clients.get_mut(&client_id).unwrap();
            client.last_heartbeat =
                Utc::now() - Duration::try_seconds(10).unwrap();
        }

        registry.update_statuses();
        let clients = registry.list_clients();
        assert_eq!(clients[0].status, ClientStatus::Online);

        // Set to 20 seconds ago (should be Offline - >= 15s)
        {
            let mut clients = registry.clients.write().unwrap();
            let client = clients.get_mut(&client_id).unwrap();
            client.last_heartbeat =
                Utc::now() - Duration::try_seconds(20).unwrap();
        }

        registry.update_statuses();
        let clients = registry.list_clients();
        assert_eq!(clients[0].status, ClientStatus::Offline);
    }

    #[test]
    fn test_registry_time_connected_zero_when_offline() {
        let registry = Registry::new();
        let info = create_test_client_info("testhost");

        let client_id = registry.register(info);

        // Client should be online with non-zero time connected
        let clients = registry.list_clients();
        assert_eq!(clients[0].status, ClientStatus::Online);
        assert!(clients[0].time_connected().num_seconds() >= 0);

        // Set heartbeat to old time to trigger offline
        {
            let mut clients = registry.clients.write().unwrap();
            let client = clients.get_mut(&client_id).unwrap();
            client.last_heartbeat =
                Utc::now() - Duration::try_seconds(20).unwrap();
        }

        // Update statuses - should transition to offline with zero time connected
        registry.update_statuses();
        let clients = registry.list_clients();
        assert_eq!(clients[0].status, ClientStatus::Offline);
        assert_eq!(
            clients[0].time_connected().num_seconds(),
            0,
            "Time connected should be exactly zero for offline clients"
        );
    }

    #[test]
    fn test_registry_multiple_clients() {
        let registry = Registry::new();

        let info1 = create_test_client_info("host1");
        let info2 = create_test_client_info("host2");
        let info3 = create_test_client_info("host3");

        registry.register(info1);
        registry.register(info2);
        registry.register(info3);

        let clients = registry.list_clients();
        assert_eq!(clients.len(), 3);

        // Verify all have unique IDs
        let mut ids: Vec<_> = clients.iter().map(|c| c.client_id).collect();
        ids.sort_by_key(|id| id.0.as_u128());
        ids.dedup();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn test_registry_clone_shares_data() {
        let registry1 = Registry::new();
        let info = create_test_client_info("testhost");

        registry1.register(info);

        // Clone registry
        let registry2 = registry1.clone();

        // Both should see the same client
        assert_eq!(registry1.list_clients().len(), 1);
        assert_eq!(registry2.list_clients().len(), 1);

        // Register via clone
        let info2 = create_test_client_info("host2");
        registry2.register(info2);

        // Both should see both clients
        assert_eq!(registry1.list_clients().len(), 2);
        assert_eq!(registry2.list_clients().len(), 2);
    }
}
