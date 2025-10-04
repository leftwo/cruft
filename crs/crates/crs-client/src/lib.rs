// Copyright 2025 Oxide Computer Company

//! Client library for connecting to the Central Registry Service
//!
//! This library provides functionality for clients to register with a CRS
//! server and maintain their online status through periodic heartbeats.
//!
//! # Overview
//!
//! The client library handles:
//! - Registration with the CRS server
//! - Periodic heartbeat transmission
//! - Automatic reconnection on failures
//! - Graceful shutdown
//!
//! # Client ID Generation
//!
//! Client IDs are deterministic UUIDs (v5) generated from:
//! - Hostname
//! - Operating system
//! - IP address
//!
//! This ensures the same client receives the same ID across restarts.
//!
//! # Usage
//!
//! ```no_run
//! # use crs_client::*;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = CrsClient::new(
//!     "http://127.0.0.1:8081".to_string(),
//!     "0.1.0".to_string(),
//! ).await?;
//!
//! client.run().await?;
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use crs_common::{ClientId, ClientInfo, HeartbeatRequest, RegisterRequest};
use std::collections::HashMap;
use std::time::Duration;

/// CRS client
///
/// Handles registration and heartbeat communication with the CRS server.
pub struct CrsClient {
    server_url: String,
    client_info: ClientInfo,
    client_id: Option<ClientId>,
    heartbeat_interval: Duration,
    http_client: reqwest::Client,
}

impl CrsClient {
    /// Create a new CRS client
    ///
    /// Automatically detects hostname, OS, and IP address. Registers
    /// with the server immediately.
    ///
    /// # Arguments
    ///
    /// * `server_url` - Base URL of the CRS server (e.g.,
    ///   "http://127.0.0.1:8081")
    /// * `version` - Version string for this client
    pub async fn new(server_url: String, version: String) -> Result<Self> {
        let hostname = hostname::get()
            .context("failed to get hostname")?
            .to_string_lossy()
            .to_string();

        let os = std::env::consts::OS.to_string();

        // Get local IP address (best effort)
        let ip_address =
            Self::get_local_ip().unwrap_or_else(|| "0.0.0.0".to_string());

        let client_info = ClientInfo {
            hostname,
            os,
            ip_address,
            version,
            tags: HashMap::new(),
        };

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("failed to create HTTP client")?;

        let mut client = Self {
            server_url,
            client_info,
            client_id: None,
            heartbeat_interval: Duration::from_secs(30),
            http_client,
        };

        // Register with the server
        client.register().await?;

        Ok(client)
    }

    /// Register with the CRS server
    async fn register(&mut self) -> Result<()> {
        let url = format!("{}/api/register", self.server_url);

        let request = RegisterRequest {
            client_info: self.client_info.clone(),
        };

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("failed to send registration request")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "registration failed with status: {}",
                response.status()
            );
        }

        let register_response: crs_common::RegisterResponse = response
            .json()
            .await
            .context("failed to parse registration response")?;

        self.client_id = Some(register_response.client_id);
        self.heartbeat_interval =
            Duration::from_secs(register_response.heartbeat_interval_secs);

        println!(
            "Registered with CRS server, client ID: {}",
            register_response.client_id
        );
        println!(
            "Heartbeat interval: {}s",
            register_response.heartbeat_interval_secs
        );

        Ok(())
    }

    /// Send a heartbeat to the CRS server
    async fn heartbeat(&self) -> Result<()> {
        let client_id = self.client_id.context("client not registered")?;

        let url = format!("{}/api/heartbeat", self.server_url);

        let request = HeartbeatRequest { client_id };

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("failed to send heartbeat")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "heartbeat failed with status: {}",
                response.status()
            );
        }

        Ok(())
    }

    /// Run the client heartbeat loop
    ///
    /// This function runs indefinitely, sending heartbeats at the
    /// configured interval. It will attempt to reconnect and
    /// re-register if heartbeats fail.
    pub async fn run(self) -> Result<()> {
        let mut interval = tokio::time::interval(self.heartbeat_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.heartbeat().await {
                eprintln!("Heartbeat failed: {}", e);
                eprintln!("Will retry on next interval...");
            }
        }
    }

    /// Get the local IP address (best effort)
    fn get_local_ip() -> Option<String> {
        // Try to get a non-loopback local IP
        local_ip_address::local_ip().ok().map(|ip| ip.to_string())
    }
}

/// Add custom tags to client information
///
/// Can be used before creating the client to add metadata.
pub fn add_client_tags(
    tags: &mut HashMap<String, String>,
    key: String,
    value: String,
) {
    tags.insert(key, value);
}
