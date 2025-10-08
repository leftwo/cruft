use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "oxping")]
#[command(about = "Ping multiple hosts from a file")]
struct Args {
    /// Path to hosts file (host,ip format)
    #[arg(short, long)]
    file: PathBuf,
}

#[derive(Debug, Clone)]
struct Host {
    name: String,
    ip: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostStatus {
    Up,
    Down,
}

#[derive(Debug, Clone)]
struct HostResult {
    host: Host,
    status: HostStatus,
}

// History of ping results for each host
type HostHistory = HashMap<String, VecDeque<HostStatus>>;

fn parse_hosts_file(path: &PathBuf) -> Result<Vec<Host>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let reader = BufReader::new(file);

    let mut hosts = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("Error reading line: {}", e))?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            return Err(format!(
                "Invalid format at line {}: expected 'host,ip'",
                line_num + 1
            ));
        }

        let name = parts[0].trim().to_string();
        let ip: String = parts[1].trim().to_string();

        hosts.push(Host { name, ip });
    }

    if hosts.is_empty() {
        return Err("No hosts found in file".to_string());
    }

    Ok(hosts)
}

fn new_ping_host(ip: String) -> Result<bool, std::io::Error> {
    let result = Command::new("ping")
        .arg("-c")
        .arg("1") // Send 1 packet
        .arg("-W")
        .arg("5") // 5 second timeout
        .arg(ip)
        .output()?;

    Ok(result.status.success())
}

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn setup_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal before printing panic
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        // Call the original panic hook
        original_hook(panic_info);
    }));
}

fn draw_ui(results: &[HostResult], history: &HostHistory) -> io::Result<()> {
    let mut stdout = io::stdout();

    // Get terminal size
    let (width, _height) = terminal::size()?;
    let width = width as usize;

    // Ensure minimum width
    let width = width.max(40);

    // Calculate column widths
    // Find longest hostname (max 16 chars)
    let max_host_len = results
        .iter()
        .map(|r| r.host.name.len().min(16))
        .max()
        .unwrap_or(4);
    let host_width = max_host_len.max(4); // At least "Host"

    // IP is fixed at 15 chars (for IPv4 xxx.xxx.xxx.xxx)
    let ip_width = 15;

    // Timeline gets the rest
    // Format: "║ Host IP Timeline ║"
    // Borders: 2, spaces: 4 (1 after ║, 1 between cols, 1 between cols, 1 before ║)
    let timeline_width = width.saturating_sub(2 + host_width + ip_width + 4);

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // Draw top border
    let top_border = format!("╔{}╗", "═".repeat(width - 2));
    write!(stdout, "{}\r\n", top_border)?;

    // Draw title
    let title = "OxPing Monitor";
    let title_padding = (width - 2).saturating_sub(title.len()) / 2;
    write!(
        stdout,
        "║{}{}{}║\r\n",
        " ".repeat(title_padding),
        title,
        " ".repeat(width - 2 - title_padding - title.len())
    )?;

    // Draw separator
    let separator = format!("╠{}╣", "═".repeat(width - 2));
    write!(stdout, "{}\r\n", separator)?;

    // Draw header
    write!(
        stdout,
        "║ {:<host_width$} {:<ip_width$} Timeline{}║\r\n",
        "Host",
        "IP",
        " ".repeat(timeline_width.saturating_sub(8)),
        host_width = host_width,
        ip_width = ip_width
    )?;

    // Draw separator
    write!(stdout, "{}\r\n", separator)?;

    // Draw each host with timeline
    for result in results {
        // Truncate hostname to 16 chars max
        let name = if result.host.name.len() > 16 {
            &result.host.name[..16]
        } else {
            &result.host.name
        };

        // Get history for this host
        let host_history = history
            .get(&result.host.ip)
            .map(|h| h.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        // Build timeline with colored status characters
        let mut timeline = String::new();
        for status in host_history.iter().take(timeline_width) {
            let (color, chr) = match status {
                HostStatus::Up => ("\x1b[32m", "●"),   // green
                HostStatus::Down => ("\x1b[31m", "○"), // red
            };
            timeline.push_str(&format!("{}{}\x1b[0m", color, chr));
        }

        // Pad timeline if needed
        let blocks_shown = host_history.len().min(timeline_width);
        if blocks_shown < timeline_width {
            timeline.push_str(&" ".repeat(timeline_width - blocks_shown));
        }

        write!(
            stdout,
            "║ {:<host_width$} {:<ip_width$} {}║\r\n",
            name,
            result.host.ip,
            timeline,
            host_width = host_width,
            ip_width = ip_width
        )?;
    }

    // Draw bottom border
    let bottom_border = format!("╚{}╝", "═".repeat(width - 2));
    write!(stdout, "{}\r\n", bottom_border)?;
    write!(stdout, "\r\nPress Ctrl-C to exit")?;

    stdout.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    setup_panic_hook();

    let args = Args::parse();

    let hosts = match parse_hosts_file(&args.file) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Set up terminal
    let _guard = match TerminalGuard::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to initialize terminal: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize history for all hosts
    let mut history: HostHistory = HashMap::new();

    loop {
        let start = tokio::time::Instant::now();

        // Ping all hosts in parallel
        let mut tasks = Vec::new();
        for host in &hosts {
            let host_clone = host.clone();
            tasks.push(tokio::spawn(async move {
                match new_ping_host(host_clone.ip.clone()) {
                    Ok(is_up) => {
                        let status = if is_up {
                            HostStatus::Up
                        } else {
                            HostStatus::Down
                        };
                        Ok(HostResult {
                            host: host_clone,
                            status,
                        })
                    }
                    Err(e) => Err((host_clone, e.to_string())),
                }
            }));
        }

        // Wait for all pings to complete
        let mut results = Vec::new();
        for task in tasks {
            match task.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err((host, _err))) => {
                    // Still show the host as down
                    results.push(HostResult {
                        host,
                        status: HostStatus::Down,
                    });
                }
                Err(_) => {}
            }
        }

        // Update history with new results (add to front, newest on left)
        for result in &results {
            let entry = history.entry(result.host.ip.clone()).or_default();
            entry.push_front(result.status);
            // Keep only what fits on screen (no need to store more)
            if entry.len() > 200 {
                entry.pop_back();
            }
        }

        // Draw the UI
        if let Err(e) = draw_ui(&results, &history) {
            eprintln!("Error drawing UI: {}", e);
            break;
        }

        // Wait for next interval or check for Ctrl-C
        let elapsed = start.elapsed();
        let sleep_duration = if elapsed < Duration::from_secs(15) {
            Duration::from_secs(15) - elapsed
        } else {
            Duration::from_millis(100)
        };

        // Check for keyboard events during the sleep period
        let start_sleep = tokio::time::Instant::now();
        let mut should_exit = false;

        while start_sleep.elapsed() < sleep_duration {
            // Poll for keyboard event with short timeout
            let poll_result = tokio::task::spawn_blocking(|| {
                if event::poll(Duration::from_millis(100)).unwrap_or(false)
                    && let Ok(Event::Key(key_event)) = event::read()
                        && key_event.code == KeyCode::Char('c')
                            && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            return true;
                        }
                false
            })
            .await;

            if poll_result.unwrap_or(false) {
                should_exit = true;
                break;
            }
        }

        if should_exit {
            break;
        }
    }
}
