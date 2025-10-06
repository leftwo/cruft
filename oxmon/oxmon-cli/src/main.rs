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

    let mut hosts: Vec<HostStatus> = response.json().await?;

    if hosts.is_empty() {
        println!("No hosts configured");
        return Ok(());
    }

    // Sort by IP address
    hosts.sort_by_key(|host| host.ip_address);

    // Print table header
    println!("{:<20} {:<16} {:<10}", "HOSTNAME", "IP ADDRESS", "STATUS");
    println!("{}", "-".repeat(46));

    // Print each host
    for host in hosts {
        let status_str = match host.status {
            Status::Online => "on",
            Status::Offline => "off",
        };

        println!(
            "{:<20} {:<16} {:<10}",
            host.hostname, host.ip_address, status_str
        );
    }

    Ok(())
}
