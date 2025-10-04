// Copyright 2025 Oxide Computer Company

//! CRS Client binary
//!
//! Command-line client for connecting to a Central Registry Service
//! server.

use anyhow::Result;
use clap::Parser;

/// CRS Client - Register with a Central Registry Service server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// URL of the CRS server (required)
    #[arg(short, long)]
    server: String,

    /// Client version string
    #[arg(short, long, default_value = "0.1.0")]
    version: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("CRS Client starting...");
    println!("Server: {}", args.server);
    println!("Version: {}", args.version);
    println!();

    // Create and run the client
    let client = crs_client::CrsClient::new(args.server, args.version).await?;

    println!("Starting heartbeat loop...");
    client.run().await?;

    Ok(())
}
