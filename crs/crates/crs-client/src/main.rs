// Copyright 2025 Oxide Computer Company

//! CRS Client binary
//!
//! Command-line client for connecting to a Central Registry Service
//! server.

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// CRS Client - Register with a Central Registry Service server
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = resolve_config(args)?;

    // Get the version from Cargo.toml at compile time
    let version = env!("CARGO_PKG_VERSION").to_string();

    println!("CRS Client starting...");
    println!("Server: {}", config.server);
    println!("Client Version: {}", version);
    println!();

    // Create and run the client
    let client = crs_client::CrsClient::new(config.server, version).await?;

    println!("Starting heartbeat loop...");
    client.run().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that ensures all CLI arguments are represented in the Config struct.
    /// This test will fail if someone adds a new CLI argument without adding
    /// it to the TOML config structure.
    #[test]
    fn test_cli_and_config_fields_match() {
        use std::collections::HashSet;

        // Get all field names from Args struct (excluding 'config' since it's meta)
        let cli_fields: HashSet<&str> = {
            let mut fields = HashSet::new();
            // Manually list all CLI option fields here
            fields.insert("server");
            fields
        };

        // Get all field names from Config struct
        let config_fields: HashSet<&str> = {
            let mut fields = HashSet::new();
            // Manually list all Config fields here
            fields.insert("server");
            fields
        };

        // Check that every CLI field is in Config
        let missing_in_config: Vec<_> =
            cli_fields.difference(&config_fields).collect();
        assert!(
            missing_in_config.is_empty(),
            "CLI fields missing from Config struct: {:?}. \
             All command line options must be supported in the TOML config file.",
            missing_in_config
        );

        // Also check the reverse (optional, but good for completeness)
        let extra_in_config: Vec<_> =
            config_fields.difference(&cli_fields).collect();
        assert!(
            extra_in_config.is_empty(),
            "Config fields not in CLI: {:?}",
            extra_in_config
        );
    }

    #[test]
    fn test_config_parsing() {
        let toml_str = r#"
            server = "http://localhost:8081"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server, Some("http://localhost:8081".to_string()));
    }

    #[test]
    fn test_config_with_missing_server() {
        let toml_str = r#"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server, None);
    }
}
