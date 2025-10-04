// Copyright 2025 Oxide Computer Company

//! Client registry implementation

use chrono::{Duration, Utc};
use crs_common::{ClientId, ClientInfo, ClientStatus, RegisteredClient};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Thresholds for client status transitions (in seconds)
const ONLINE_THRESHOLD_SECS: i64 = 60;
const STALE_THRESHOLD_SECS: i64 = 180;

/// Registry for tracking connected clients
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
    /// registration timestamp. The client is marked as online and the
    /// last heartbeat time is updated to now.
    pub fn register(&self, info: ClientInfo) -> ClientId {
        let client_id = info.client_id();
        let now = Utc::now();

        let mut clients = self.clients.write().unwrap();

        let registered_client = RegisteredClient {
            client_id,
            info,
            status: ClientStatus::Online,
            registered_at: clients
                .get(&client_id)
                .map(|c| c.registered_at)
                .unwrap_or(now),
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
    /// - Online: last heartbeat < 60 seconds ago
    /// - Stale: last heartbeat 60-180 seconds ago
    /// - Offline: last heartbeat > 180 seconds ago
    ///
    /// This is called periodically by a background task.
    pub fn update_statuses(&self) {
        let now = Utc::now();
        let mut clients = self.clients.write().unwrap();

        for client in clients.values_mut() {
            let elapsed = now - client.last_heartbeat;

            client.status = if elapsed
                < Duration::try_seconds(ONLINE_THRESHOLD_SECS).unwrap()
            {
                ClientStatus::Online
            } else if elapsed
                < Duration::try_seconds(STALE_THRESHOLD_SECS).unwrap()
            {
                ClientStatus::Stale
            } else {
                ClientStatus::Offline
            };
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
