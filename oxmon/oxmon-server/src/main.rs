use anyhow::Result;
use clap::Parser;
use oxmon_core::{Monitor, load_hosts_from_file};
use oxmon_db::Database;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

mod api;
mod web;

#[derive(Parser, Debug)]
#[command(name = "oxmon-server", about = "Oxide Network Monitoring Server")]
struct Args {
    /// Path to hosts file (hostname,ip_address per line)
    #[arg(short = 'f', long)]
    hosts_file: Option<PathBuf>,

    /// Bind address for HTTP server
    #[arg(short = 'b', long, default_value = "127.0.0.1")]
    bind_address: String,

    /// Bind port for HTTP server
    #[arg(short = 'p', long, default_value = "8082")]
    bind_port: u16,

    /// Path to SQLite database
    #[arg(short = 'd', long, default_value = "oxmon.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load hosts from file if provided
    let hosts = if let Some(hosts_file) = &args.hosts_file {
        println!("Loading hosts from {}...", hosts_file.display());
        let hosts = load_hosts_from_file(hosts_file)?;
        println!("Loaded {} hosts", hosts.len());
        hosts
    } else {
        println!("No hosts file provided, using existing database hosts");
        Vec::new()
    };

    // Initialize database
    let (db, is_new) = Database::new(&args.db_path).await?;
    if is_new {
        println!("Creating new database at {}...", args.db_path);
    } else {
        println!("Loading existing database from {}...", args.db_path);
    }
    let db = Arc::new(db);

    // Create monitor
    println!("Starting monitor...");
    let monitor = Arc::new(Monitor::new(db, hosts).await?);

    // Start monitoring task in background
    let monitor_clone = monitor.clone();
    tokio::spawn(async move {
        if let Err(e) = monitor_clone.start().await {
            eprintln!("Monitor error: {}", e);
        }
    });

    // Start HTTP server
    let addr: SocketAddr =
        format!("{}:{}", args.bind_address, args.bind_port).parse()?;

    println!("Starting HTTP server on http://{}...", addr);
    println!("Dashboard: http://{}/", addr);
    println!("API: http://{}/api/hosts", addr);

    api::start_server(addr, monitor).await?;

    Ok(())
}
