use anyhow::Result;
use clap::Parser;
use oxmon_common::{HostTimeline, Status, TimelineBucketState};

#[derive(Parser, Debug)]
#[command(name = "oxmon", about = "Oxide Network Monitoring CLI", version)]
struct Args {
    /// Server URL
    #[arg(short = 's', long, default_value = "http://127.0.0.1:8082")]
    server_url: String,

    /// Terminal width (defaults to auto-detect)
    #[arg(short = 'w', long)]
    width: Option<usize>,

    /// Duration in hours to look back (default: 2)
    #[arg(short = 'd', long, default_value = "2")]
    duration_hours: u32,

    /// Number of time buckets (default: 20)
    #[arg(short = 'b', long, default_value = "20")]
    num_buckets: usize,

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
        Command::List => {
            list_hosts(
                &args.server_url,
                args.width,
                args.duration_hours,
                args.num_buckets,
            )
            .await?
        }
    }

    Ok(())
}

/// Get terminal width, either from user arg or auto-detect
fn get_terminal_width(width_arg: Option<usize>) -> usize {
    if let Some(width) = width_arg {
        return width;
    }

    // Auto-detect terminal width
    if let Some((terminal_size::Width(w), _)) = terminal_size::terminal_size() {
        w as usize
    } else {
        // Fallback to 80 columns if detection fails
        80
    }
}

async fn list_hosts(
    server_url: &str,
    width_arg: Option<usize>,
    duration_hours: u32,
    num_buckets: usize,
) -> Result<()> {
    let url = format!(
        "{}/api/timelines?duration_hours={}&num_buckets={}",
        server_url, duration_hours, num_buckets
    );
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

    // Calculate column widths
    let terminal_width = get_terminal_width(width_arg);
    const HOSTNAME_WIDTH: usize = 16;
    const IP_WIDTH: usize = 15;
    const STATUS_WIDTH: usize = 3;

    // Fixed columns take: 16 (hostname) + 1 (space) + 15 (ip)
    // + 1 (space) + 3 (status) + 2 (spaces) = 38
    const FIXED_COLUMNS: usize = HOSTNAME_WIDTH + IP_WIDTH + STATUS_WIDTH + 4;

    // Print table header
    let history_label = format!("HISTORY (Past {}h)", duration_hours);
    println!(
        "{:<16} {:<15} {:<3} {}",
        "HOSTNAME", "IP ADDRESS", "STA", history_label
    );
    println!("{}", "-".repeat(terminal_width));

    // Print each host
    for timeline in timelines {
        let status_str = match timeline.current_status {
            Status::Online => "on",
            Status::Offline => "off",
        };

        // Truncate hostname to 16 chars
        let hostname = if timeline.hostname.len() > 16 {
            &timeline.hostname[..16]
        } else {
            &timeline.hostname
        };

        // Render history - calculate how many buckets fit
        let available_width = terminal_width.saturating_sub(FIXED_COLUMNS);
        let timeline_str = render_timeline(&timeline.buckets, available_width);

        println!(
            "{:<16} {:<15} {:<3} {}",
            hostname, timeline.ip_address, status_str, timeline_str
        );
    }

    Ok(())
}

fn render_timeline(
    buckets: &[TimelineBucketState],
    max_width: usize,
) -> String {
    buckets
        .iter()
        .take(max_width)
        .map(|state| match state {
            TimelineBucketState::Online => '-',
            TimelineBucketState::Offline => '_',
            TimelineBucketState::NoData => '·',
        })
        .collect()
}
