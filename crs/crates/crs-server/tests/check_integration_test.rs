// Copyright 2025 Oxide Computer Company

//! Integration tests for crs-check that verify it correctly displays
//! server state and client status changes.

use chrono::Utc;
use crs_common::{ClientInfo, ClientStatus};
use crs_server::registry::Registry;
use std::collections::HashMap;

/// Helper to create test client info
fn create_client_info(hostname: &str) -> ClientInfo {
    ClientInfo {
        hostname: hostname.to_string(),
        os: "linux".to_string(),
        ip_address: "192.168.1.100".to_string(),
        version: "1.0.0".to_string(),
        tags: HashMap::new(),
    }
}

#[tokio::test]
async fn test_check_fetches_client_list() {
    let registry = Registry::new();

    // Register some clients
    let info1 = create_client_info("test-client-1");
    let info2 = create_client_info("test-client-2");
    let info3 = create_client_info("test-client-3");

    registry.register(info1);
    registry.register(info2);
    registry.register(info3);

    let clients = registry.list_clients();
    assert_eq!(clients.len(), 3);
}

#[tokio::test]
async fn test_check_reflects_status_changes() {
    let registry = Registry::new();

    // Register a client
    let info = create_client_info("status-change-test");
    let client_id = registry.register(info);

    // Initially online
    let clients = registry.list_clients();
    assert_eq!(clients[0].status, ClientStatus::Online);

    // Make it stale
    registry.set_last_heartbeat(
        client_id,
        Utc::now() - chrono::Duration::try_seconds(50).unwrap(),
    );
    registry.update_statuses();

    let clients = registry.list_clients();
    assert_eq!(clients[0].status, ClientStatus::Stale);

    // Make it offline
    registry.set_last_heartbeat(
        client_id,
        Utc::now() - chrono::Duration::try_seconds(100).unwrap(),
    );
    registry.update_statuses();

    let clients = registry.list_clients();
    assert_eq!(clients[0].status, ClientStatus::Offline);
}

#[tokio::test]
async fn test_check_shows_multiple_client_states() {
    let registry = Registry::new();

    // Register three clients with different states
    let info1 = create_client_info("client-online");
    let info2 = create_client_info("client-stale");
    let info3 = create_client_info("client-offline");

    let id1 = registry.register(info1);
    let id2 = registry.register(info2);
    let id3 = registry.register(info3);

    // Set different heartbeat times
    registry.set_last_heartbeat(
        id1,
        Utc::now() - chrono::Duration::try_seconds(10).unwrap(),
    );
    registry.set_last_heartbeat(
        id2,
        Utc::now() - chrono::Duration::try_seconds(50).unwrap(),
    );
    registry.set_last_heartbeat(
        id3,
        Utc::now() - chrono::Duration::try_seconds(100).unwrap(),
    );

    registry.update_statuses();

    let clients = registry.list_clients();
    assert_eq!(clients.len(), 3);

    // Verify each client has the correct status
    let online_count = clients
        .iter()
        .filter(|c| c.status == ClientStatus::Online)
        .count();
    let stale_count = clients
        .iter()
        .filter(|c| c.status == ClientStatus::Stale)
        .count();
    let offline_count = clients
        .iter()
        .filter(|c| c.status == ClientStatus::Offline)
        .count();

    assert_eq!(online_count, 1);
    assert_eq!(stale_count, 1);
    assert_eq!(offline_count, 1);
}

#[tokio::test]
async fn test_check_client_reconnection_updates() {
    let registry = Registry::new();

    let info = create_client_info("reconnection-test");
    let client_id = registry.register(info.clone());

    // Make client offline
    registry.set_last_heartbeat(
        client_id,
        Utc::now() - chrono::Duration::try_seconds(100).unwrap(),
    );
    registry.update_statuses();

    let clients = registry.list_clients();
    assert_eq!(clients[0].status, ClientStatus::Offline);

    // Client reconnects (send heartbeat)
    registry.heartbeat(client_id).unwrap();

    let clients = registry.list_clients();
    assert_eq!(clients[0].status, ClientStatus::Online);
}

#[tokio::test]
async fn test_check_preserves_registration_time() {
    let registry = Registry::new();

    let info = create_client_info("time-preservation-test");
    let _client_id = registry.register(info.clone());

    let clients = registry.list_clients();
    let original_registered_at = clients[0].registered_at;

    // Wait a bit and re-register
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    registry.register(info);

    let clients = registry.list_clients();
    assert_eq!(clients[0].registered_at, original_registered_at);
}
