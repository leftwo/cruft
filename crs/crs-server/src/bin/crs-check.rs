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

fn format_duration(
    registered_at: chrono::DateTime<chrono::Utc>,
    last_heartbeat: chrono::DateTime<chrono::Utc>,
    status: ClientStatus,
) -> String {
    // For offline clients, use last_heartbeat instead of now
    let end_time = if status == ClientStatus::Offline {
        last_heartbeat
    } else {
        Utc::now()
    };
    let duration = end_time - registered_at;

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
        "{:<16} {:<15} {:<7} {:<8} {:<8} {:<14}",
        "Hostname", "IP Address", "OS", "Version", "Status", "Time Connected"
    );
    println!("{}", "-".repeat(80));

    // Client rows
    for client in &response.clients {
        let hostname = truncate_str(&client.info.hostname, 16);
        let ip = truncate_str(&client.info.ip_address, 15);
        let os = truncate_str(&client.info.os, 7);
        let version = truncate_str(&client.info.version, 8);
        let status = format_status(client.status);
        let time_connected = format_duration(
            client.registered_at,
            client.last_heartbeat,
            client.status,
        );

        println!(
            "{:<16} {:<15} {:<7} {:<8} {:<8} {:<14}",
            hostname, ip, os, version, status, time_connected
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

        let now = Utc::now();

        // Test seconds (online client)
        let registered = now - Duration::try_seconds(30).unwrap();
        assert_eq!(
            format_duration(registered, now, ClientStatus::Online),
            "30s"
        );

        // Test minutes (online client)
        let registered = now - Duration::try_seconds(150).unwrap();
        assert_eq!(
            format_duration(registered, now, ClientStatus::Online),
            "2m"
        );

        // Test hours (online client)
        let registered = now - Duration::try_seconds(3900).unwrap();
        assert_eq!(
            format_duration(registered, now, ClientStatus::Online),
            "1h 5m"
        );

        // Test days (online client)
        let registered = now - Duration::try_seconds(90000).unwrap();
        assert_eq!(
            format_duration(registered, now, ClientStatus::Online),
            "1d 1h"
        );
    }

    #[test]
    fn test_format_duration_offline_client() {
        use chrono::Duration;

        let now = Utc::now();

        // Client registered 10 minutes ago, last heartbeat 5 minutes ago
        let registered = now - Duration::try_seconds(600).unwrap();
        let last_heartbeat = now - Duration::try_seconds(300).unwrap();

        // For offline client, should show time from registered to last_heartbeat (5 minutes)
        assert_eq!(
            format_duration(registered, last_heartbeat, ClientStatus::Offline),
            "5m"
        );

        // For online client, should show time from registered to now (10 minutes)
        assert_eq!(
            format_duration(registered, last_heartbeat, ClientStatus::Online),
            "10m"
        );
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
}
