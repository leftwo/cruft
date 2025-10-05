use anyhow::Result;
use clap::Parser;
use oxmon_common::{HostStatus, Status};

#[derive(Parser, Debug)]
#[command(name = "oxmon", about = "Oxide Network Monitoring CLI", version)]
struct Args {
    /// Server URL
    #[arg(short = 's', long, default_value = "http://127.0.0.1:8082")]
    server_url: String,

    /// Command to execute
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Parser, Debug)]
enum Command {
    /// List all hosts and their current status
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Default to list command if none specified
    let command = args.command.unwrap_or(Command::List);

    match command {
        Command::List => list_hosts(&args.server_url).await?,
    }

    Ok(())
}

async fn list_hosts(server_url: &str) -> Result<()> {
    let url = format!("{}/api/hosts", server_url);
    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        anyhow::bail!("Server returned error: {}", response.status());
    }

    let hosts: Vec<HostStatus> = response.json().await?;

    if hosts.is_empty() {
        println!("No hosts configured");
        return Ok(());
    }

    // Print table header
    println!(
        "{:<20} {:<16} {:<10} {:<12} {:<10}",
        "HOSTNAME", "IP ADDRESS", "STATUS", "SUCCESS", "LATENCY"
    );
    println!("{}", "-".repeat(72));

    // Print each host
    for host in hosts {
        let status_str = match host.status {
            Status::Online => "✓ Online",
            Status::Offline => "✗ Offline",
        };

        let success_rate =
            format!("{}/{}", host.success_count, host.total_count);

        let latency_str = host
            .avg_latency_ms
            .map(|l| format!("{:.1}ms", l))
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<20} {:<16} {:<10} {:<12} {:<10}",
            host.hostname,
            host.ip_address,
            status_str,
            success_rate,
            latency_str
        );
    }

    Ok(())
}
