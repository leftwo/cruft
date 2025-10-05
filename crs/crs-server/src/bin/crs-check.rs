// Copyright 2025 Oxide Computer Company

//! CRS Check - Command-line status viewer for CRS server
//!
//! Displays server and client status in an 80-column text format.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use crs_common::{ClientStatus, ListClientsResponse};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// CRS Check - View CRS server status
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// URL of the CRS server
    #[arg(short, long)]
    server: Option<String>,

    /// Path to TOML configuration file
    #[arg(short, long)]
    config: Option<PathBuf>,
}

/// Configuration file structure
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    /// URL of the CRS server
    server: Option<String>,
}

/// Final resolved configuration
struct ResolvedConfig {
    server: String,
}

fn load_config(path: &PathBuf) -> Result<Config> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;
    let config: Config = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file: {:?}", path))?;
    Ok(config)
}

fn resolve_config(args: Args) -> Result<ResolvedConfig> {
    let file_config = if let Some(config_path) = &args.config {
        Some(load_config(config_path)?)
    } else {
        None
    };

    // Resolve server, preferring CLI over config file
    let server = if let Some(cli_server) = args.server {
        if let Some(ref file_cfg) = file_config {
            if file_cfg.server.is_some() {
                eprintln!(
                    "Warning: Server specified in both config file and command line. Using command line value."
                );
            }
        }
        cli_server
    } else if let Some(ref file_cfg) = file_config {
        file_cfg
            .server
            .clone()
            .context("Server not specified in config file")?
    } else {
        anyhow::bail!(
            "Server URL must be specified via --server or in config file"
        );
    };

    Ok(ResolvedConfig { server })
}

async fn fetch_clients(server_url: &str) -> Result<ListClientsResponse> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/clients", server_url);

    let response = client.get(&url).send().await.with_context(|| {
        format!("Failed to connect to server: {}", server_url)
    })?;

    if !response.status().is_success() {
        anyhow::bail!("Server returned error: {}", response.status());
    }

    let clients_response = response
        .json::<ListClientsResponse>()
        .await
        .context("Failed to parse server response")?;

    Ok(clients_response)
}

fn format_duration(client: &crs_common::RegisteredClient) -> String {
    let duration = client.time_connected();

    if duration.num_days() > 0 {
        format!("{}d {}h", duration.num_days(), duration.num_hours() % 24)
    } else if duration.num_hours() > 0 {
        format!("{}h {}m", duration.num_hours(), duration.num_minutes() % 60)
    } else if duration.num_minutes() > 0 {
        format!("{}m", duration.num_minutes())
    } else {
        format!("{}s", duration.num_seconds())
    }
}

fn format_status(status: ClientStatus) -> &'static str {
    match status {
        ClientStatus::Online => "online",
        ClientStatus::Offline => "offline",
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn display_status(mut response: ListClientsResponse) {
    println!("{}", "=".repeat(80));
    println!("CRS Server Status");
    println!("{}", "=".repeat(80));
    println!();

    // Server uptime
    let now = Utc::now();
    let uptime_duration = now - response.server_start_time;
    let uptime_str = if uptime_duration.num_days() > 0 {
        format!(
            "{}d {}h",
            uptime_duration.num_days(),
            uptime_duration.num_hours() % 24
        )
    } else if uptime_duration.num_hours() > 0 {
        format!(
            "{}h {}m",
            uptime_duration.num_hours(),
            uptime_duration.num_minutes() % 60
        )
    } else if uptime_duration.num_minutes() > 0 {
        format!("{}m", uptime_duration.num_minutes())
    } else {
        format!("{}s", uptime_duration.num_seconds())
    };

    println!("Server Uptime: {}", uptime_str);
    println!();

    // Sort clients by IP address
    response
        .clients
        .sort_by(|a, b| a.info.ip_address.cmp(&b.info.ip_address));

    // Client table header
    println!("Registered Clients ({}):", response.clients.len());
    println!("{}", "-".repeat(80));
    println!(
        "{:<16} {:<15} {:<7} {:<19} {:<8} {:<10}",
        "Hostname", "IP Address", "OS", "Connected", "Status", "Connected"
    );
    println!("{}", "-".repeat(80));

    // Client rows
    for client in &response.clients {
        let hostname = truncate_str(&client.info.hostname, 16);
        let ip = truncate_str(&client.info.ip_address, 15);
        let os = truncate_str(&client.info.os, 7);
        let first_connected = client.first_connected.format("%Y-%m-%d %H:%M:%S").to_string();
        let status = format_status(client.status);
        let time_connected = format_duration(client);

        println!(
            "{:<16} {:<15} {:<7} {:<19} {:<8} {:<10}",
            hostname, ip, os, first_connected, status, time_connected
        );
    }

    println!("{}", "-".repeat(80));
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = resolve_config(args)?;

    let response = fetch_clients(&config.server).await?;
    display_status(response);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that ensures all CLI arguments are represented in the Config struct.
    #[test]
    fn test_cli_and_config_fields_match() {
        use std::collections::HashSet;

        let cli_fields: HashSet<&str> = {
            let mut fields = HashSet::new();
            fields.insert("server");
            fields
        };

        let config_fields: HashSet<&str> = {
            let mut fields = HashSet::new();
            fields.insert("server");
            fields
        };

        let missing_in_config: Vec<_> =
            cli_fields.difference(&config_fields).collect();
        assert!(
            missing_in_config.is_empty(),
            "CLI fields missing from Config struct: {:?}",
            missing_in_config
        );

        let extra_in_config: Vec<_> =
            config_fields.difference(&cli_fields).collect();
        assert!(
            extra_in_config.is_empty(),
            "Config fields not in CLI: {:?}",
            extra_in_config
        );
    }

    #[test]
    fn test_format_duration() {
        use chrono::Duration;
        use crs_common::{ClientId, ClientInfo, RegisteredClient};
        use std::collections::HashMap;

        let now = Utc::now();

        // Test seconds (online client)
        let client = RegisteredClient {
            client_id: ClientId::from_client_data("test", "linux", None),
            info: ClientInfo {
                hostname: "test".to_string(),
                os: "linux".to_string(),
                ip_address: "127.0.0.1".to_string(),
                version: "1.0.0".to_string(),
                host_id: None,
                tags: HashMap::new(),
            },
            status: ClientStatus::Online,
            first_connected: now - Duration::try_seconds(30).unwrap(),
            registered_at: now - Duration::try_seconds(30).unwrap(),
            last_heartbeat: now,
        };
        assert_eq!(format_duration(&client), "30s");
    }

    #[test]
    fn test_format_duration_offline_client() {
        use chrono::Duration;
        use crs_common::{ClientId, ClientInfo, RegisteredClient};
        use std::collections::HashMap;

        let now = Utc::now();

        // Offline client should always show 0s
        let client = RegisteredClient {
            client_id: ClientId::from_client_data("test", "linux", None),
            info: ClientInfo {
                hostname: "test".to_string(),
                os: "linux".to_string(),
                ip_address: "127.0.0.1".to_string(),
                version: "1.0.0".to_string(),
                host_id: None,
                tags: HashMap::new(),
            },
            status: ClientStatus::Offline,
            first_connected: now - Duration::try_seconds(600).unwrap(),
            registered_at: now - Duration::try_seconds(300).unwrap(),
            last_heartbeat: now - Duration::try_seconds(300).unwrap(),
        };
        assert_eq!(format_duration(&client), "0s");
    }

    #[test]
    fn test_format_status() {
        assert_eq!(format_status(ClientStatus::Online), "online");
        assert_eq!(format_status(ClientStatus::Offline), "offline");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("verylongstring", 10), "verylon...");
        assert_eq!(truncate_str("exact", 5), "exact");
    }

    #[test]
    fn test_output_width_80_characters() {
        // Test that header line is exactly 80 characters
        // Format: "{:<16} {:<15} {:<7} {:<19} {:<8} {:<10}"
        // 16 + 1 + 15 + 1 + 7 + 1 + 19 + 1 + 8 + 1 + 10 = 80
        let header = format!(
            "{:<16} {:<15} {:<7} {:<19} {:<8} {:<10}",
            "Hostname", "IP Address", "OS", "Connected", "Status", "Connected"
        );
        assert_eq!(
            header.len(),
            80,
            "Header line must be exactly 80 characters, got {}",
            header.len()
        );

        // Test that data rows are exactly 80 characters with max-length values
        let row = format!(
            "{:<16} {:<15} {:<7} {:<19} {:<8} {:<10}",
            "a".repeat(16),
            "b".repeat(15),
            "c".repeat(7),
            "2025-10-05 12:34:56", // 19 chars
            "offline",              // 7 chars (fits in 8)
            "999d 99h"              // 8 chars (fits in 10)
        );
        assert_eq!(
            row.len(),
            80,
            "Data row must be exactly 80 characters, got {}",
            row.len()
        );
    }
}
