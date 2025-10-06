use anyhow::Result;
use clap::Parser;
use oxmon_common::{HostTimeline, Status, TimelineBucketState};

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
    /// List all hosts and their current status with timeline
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
    let url = format!("{}/api/timelines", server_url);
    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        anyhow::bail!("Server returned error: {}", response.status());
    }

    let mut timelines: Vec<HostTimeline> = response.json().await?;

    if timelines.is_empty() {
        println!("No hosts configured");
        return Ok(());
    }

    // Sort by IP address
    timelines.sort_by_key(|t| t.ip_address);

    // Print table header
    println!(
        "{:<20} {:<16} {:<10} HISTORY (Past 2h)",
        "HOSTNAME", "IP ADDRESS", "STATUS"
    );
    println!("{}", "-".repeat(68));

    // Print each host
    for timeline in timelines {
        let status_str = match timeline.current_status {
            Status::Online => "on",
            Status::Offline => "off",
        };

        let timeline_str = render_timeline(&timeline.buckets);

        println!(
            "{:<20} {:<16} {:<10} {}",
            timeline.hostname, timeline.ip_address, status_str, timeline_str
        );
    }

    Ok(())
}

fn render_timeline(buckets: &[TimelineBucketState]) -> String {
    buckets
        .iter()
        .map(|state| match state {
            TimelineBucketState::Online => '█',
            TimelineBucketState::Offline => '░',
            TimelineBucketState::NoData => '·',
        })
        .collect()
}
