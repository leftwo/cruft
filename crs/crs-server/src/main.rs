// Copyright 2025 Oxide Computer Company

//! Central Registry Service (CRS) Server
//!
//! The CRS server is a centralized service that tracks client connections
//! and status. Clients register themselves with the server, providing
//! information such as hostname, OS, IP address, and version. The server
//! tracks which clients are online based on periodic heartbeats.
//!
//! # Features
//!
//! - **REST API endpoints** for client registration and heartbeat reporting
//! - **Web dashboard** showing all registered clients and their status
//! - **Automatic status updates** (online/stale/offline) based on heartbeat age
//! - **Deterministic client IDs** generated from hostname and OS
//!
//! # Usage
//!
//! Start the server:
//! ```bash
//! cargo run --bin crs-server
//! ```
//!
//! The server listens on `127.0.0.1:8081` by default.
//!
//! # API Endpoints
//!
//! - `POST /api/register` - Register a new client
//! - `POST /api/heartbeat` - Send heartbeat from registered client
//! - `GET /api/clients` - List all registered clients
//! - `GET /` - Web dashboard
//!
//! # Client Status
//!
//! Clients are automatically categorized based on their last heartbeat:
//! - **Online**: Last heartbeat < 20 seconds ago (< 2x heartbeat interval)
//! - **Stale**: Last heartbeat 20-30 seconds ago (2-3x heartbeat interval)
//! - **Offline**: Last heartbeat > 30 seconds ago (> 3x heartbeat interval)
//!
//! Heartbeat interval is 10 seconds.
//! Status updates occur every 30 seconds via a background task.

mod api;
mod registry;
mod web;

use anyhow::Result;
use api::ApiContext;
use clap::Parser;
use dropshot::{
    ConfigDropshot, ConfigLogging, ConfigLoggingLevel, HttpServerStarter,
};
use registry::Registry;
use std::net::SocketAddr;
use std::time::Duration;

/// CRS Server - Central Registry Service
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// IP address to bind to
    #[arg(short, long, default_value = "127.0.0.1")]
    server_address: String,

    /// Port to listen on
    #[arg(short, long, default_value = "8081")]
    port: u16,
}

/// Main entry point for the CRS server
///
/// Initializes the registry, starts a background task to update client
/// statuses, configures and starts the HTTP server with REST API endpoints
/// and web dashboard.
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Record server start time
    let start_time = chrono::Utc::now();

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
    let bind_address: SocketAddr =
        format!("{}:{}", args.server_address, args.port)
            .parse()
            .expect("failed to parse bind address");
    let config = ConfigDropshot {
        bind_address,
        request_body_max_bytes: 1024 * 1024, // 1MB
        default_handler_task_mode: dropshot::HandlerTaskMode::Detached,
        log_headers: vec![],
    };

    // Create API context
    let context = ApiContext {
        registry,
        start_time,
    };

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
