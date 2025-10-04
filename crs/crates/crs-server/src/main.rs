// Copyright 2025 Oxide Computer Company

//! Central Registry Service (CRS) Server
//!
//! The CRS server is a centralized service that tracks client connections
//! and status. Clients register themselves with the server, providing
//! information such as hostname, OS, IP address, and version. The server
//! tracks which clients are online based on periodic heartbeats.
//!
//! The server provides:
//! - REST API endpoints for client registration and heartbeat reporting
//! - A web dashboard showing all registered clients and their status
//! - Automatic status updates (online/stale/offline) based on heartbeat age
//!
//! The server listens on 127.0.0.1:8081 by default.

mod api;
mod registry;
mod web;

use anyhow::Result;
use api::ApiContext;
use dropshot::{
    ConfigDropshot, ConfigLogging, ConfigLoggingLevel, HttpServerStarter,
};
use registry::Registry;
use std::net::SocketAddr;
use std::time::Duration;

/// Main entry point for the CRS server
///
/// Initializes the registry, starts a background task to update client
/// statuses, configures and starts the HTTP server with REST API endpoints
/// and web dashboard.
#[tokio::main]
async fn main() -> Result<()> {
    // Create the registry
    let registry = Registry::new();

    // Start background status updater task
    let registry_clone = registry.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            registry_clone.update_statuses();
        }
    });

    // Configure logging
    let log_config = ConfigLogging::StderrTerminal {
        level: ConfigLoggingLevel::Info,
    };
    let log = log_config
        .to_logger("crs-server")
        .expect("failed to create logger");

    // Configure dropshot server
    let bind_address: SocketAddr = "127.0.0.1:8081"
        .parse()
        .expect("failed to parse bind address");
    let config = ConfigDropshot {
        bind_address,
        request_body_max_bytes: 1024 * 1024, // 1MB
        default_handler_task_mode: dropshot::HandlerTaskMode::Detached,
        log_headers: vec![],
    };

    // Create API context
    let context = ApiContext { registry };

    // Build API description
    let mut api = dropshot::ApiDescription::new();
    api.register(api::register)
        .expect("failed to register endpoint");
    api.register(api::heartbeat)
        .expect("failed to register endpoint");
    api.register(api::list_clients)
        .expect("failed to register endpoint");
    api.register(web::dashboard)
        .expect("failed to register endpoint");

    // Start the server
    let server = HttpServerStarter::new(&config, api, context, &log)
        .map_err(|e| {
            eprintln!("Failed to bind to {}: {}", bind_address, e);
            std::process::exit(1);
        })
        .unwrap()
        .start();

    println!("CRS Server listening on http://{}", bind_address);
    println!("Dashboard: http://{}/", bind_address);
    println!("API endpoints:");
    println!("  POST http://{}/api/register", bind_address);
    println!("  POST http://{}/api/heartbeat", bind_address);
    println!("  GET  http://{}/api/clients", bind_address);

    server.await.map_err(|e| {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    })
}
