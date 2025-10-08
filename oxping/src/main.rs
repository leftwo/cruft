use chrono::Local;
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

// Configuration constants
const PING_INTERVAL_SECS: u64 = 10;
const TIME_MARKER_INTERVAL_SECS: u64 = 60;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryEntry {
    Status(HostStatus),
    TimeMarker, // Vertical bar separator every minute
}

#[derive(Debug, Clone)]
struct HostResult {
    host: Host,
    status: HostStatus,
}

// History of ping results for each host
type HostHistory = HashMap<String, VecDeque<HistoryEntry>>;

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

fn draw_ui(results: &[HostResult], history: &HostHistory, last_update: &str) -> io::Result<()> {
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
    // Format: "Host IP Timeline"
    // spaces: 2 (1 between cols, 1 between cols)
    let timeline_width = width.saturating_sub(host_width + ip_width + 2);

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // Draw header with timestamp on the right
    let timestamp = format!("Updated: {}", last_update);
    let header_left = format!(
        "{:<host_width$} {:<ip_width$} Timeline",
        "Host",
        "IP",
        host_width = host_width,
        ip_width = ip_width
    );
    let spacing = width
        .saturating_sub(header_left.len())
        .saturating_sub(timestamp.len());
    write!(
        stdout,
        "{}{}{}\r\n",
        header_left,
        " ".repeat(spacing),
        timestamp
    )?;

    // Draw separator
    let separator_line = "═".repeat(width);
    write!(stdout, "{}\r\n", separator_line)?;

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

        // Build timeline with colored status characters and time markers
        let mut timeline = String::new();
        for entry in host_history.iter().take(timeline_width) {
            match entry {
                HistoryEntry::Status(HostStatus::Up) => {
                    timeline.push_str("\x1b[32m●\x1b[0m"); // green
                }
                HistoryEntry::Status(HostStatus::Down) => {
                    timeline.push_str("\x1b[31m○\x1b[0m"); // red
                }
                HistoryEntry::TimeMarker => {
                    timeline.push_str("\x1b[90m|\x1b[0m"); // gray
                }
            }
        }

        // Pad timeline if needed
        let blocks_shown = host_history.len().min(timeline_width);
        if blocks_shown < timeline_width {
            timeline.push_str(&" ".repeat(timeline_width - blocks_shown));
        }

        write!(
            stdout,
            "{:<host_width$} {:<ip_width$} {}\r\n",
            name,
            result.host.ip,
            timeline,
            host_width = host_width,
            ip_width = ip_width
        )?;
    }

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

    // Calculate how many iterations before adding a time marker
    let iterations_per_marker = TIME_MARKER_INTERVAL_SECS / PING_INTERVAL_SECS;
    let mut iteration_count = 0;

    loop {
        let start = tokio::time::Instant::now();

        // Add time marker at the calculated interval
        if iteration_count > 0 && iteration_count % iterations_per_marker == 0 {
            for host in &hosts {
                let entry = history.entry(host.ip.clone()).or_default();
                entry.push_front(HistoryEntry::TimeMarker);
                if entry.len() > 200 {
                    entry.pop_back();
                }
            }
        }
        iteration_count += 1;

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
            entry.push_front(HistoryEntry::Status(result.status));
            // Keep only what fits on screen (no need to store more)
            if entry.len() > 200 {
                entry.pop_back();
            }
        }

        // Format current time for display
        let last_update = Local::now().format("%H:%M:%S").to_string();

        // Draw the UI
        if let Err(e) = draw_ui(&results, &history, &last_update) {
            eprintln!("Error drawing UI: {}", e);
            break;
        }

        // Wait for next interval or check for Ctrl-C
        let elapsed = start.elapsed();
        let target_duration = Duration::from_secs(PING_INTERVAL_SECS);
        let sleep_duration = if elapsed < target_duration {
            target_duration - elapsed
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ========== Unit Tests for parse_hosts_file ==========

    #[test]
    fn test_parse_valid_hosts() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_ok());

        let hosts = result.unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "host1");
        assert_eq!(hosts[0].ip, "192.168.1.1");
        assert_eq!(hosts[1].name, "host2");
        assert_eq!(hosts[1].ip, "10.0.0.1");
    }

    #[test]
    fn test_parse_with_whitespace() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "  host1  ,  192.168.1.1  ").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_ok());

        let hosts = result.unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "host1");
        assert_eq!(hosts[0].ip, "192.168.1.1");
    }

    #[test]
    fn test_parse_with_empty_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "   ").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_ok());

        let hosts = result.unwrap();
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn test_parse_with_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# This is a comment").unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file, "# Another comment").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_ok());

        let hosts = result.unwrap();
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn test_parse_invalid_format_missing_comma() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1 192.168.1.1").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid format at line 1"));
    }

    #[test]
    fn test_parse_invalid_format_too_many_fields() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,192.168.1.1,extra").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid format"));
    }

    #[test]
    fn test_parse_empty_file() {
        let file = NamedTempFile::new().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No hosts found"));
    }

    #[test]
    fn test_parse_only_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Comment 1").unwrap();
        writeln!(file, "# Comment 2").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No hosts found"));
    }

    #[test]
    fn test_parse_nonexistent_file() {
        let result = parse_hosts_file(&PathBuf::from("/nonexistent/file.txt"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to open file"));
    }

    // ========== Integration Tests ==========

    #[test]
    fn test_host_result_creation() {
        let host = Host {
            name: "test".to_string(),
            ip: "192.168.1.1".to_string(),
        };

        let result = HostResult {
            host: host.clone(),
            status: HostStatus::Up,
        };

        assert_eq!(result.host.name, "test");
        assert_eq!(result.host.ip, "192.168.1.1");
        assert_eq!(result.status, HostStatus::Up);
    }

    #[test]
    fn test_host_status_enum() {
        let up = HostStatus::Up;
        let down = HostStatus::Down;

        assert_eq!(up, HostStatus::Up);
        assert_eq!(down, HostStatus::Down);
        assert_ne!(up, down);
    }

    #[test]
    fn test_history_management() {
        let mut history: HostHistory = HashMap::new();
        let ip = "192.168.1.1".to_string();

        // Add some history
        let entry = history.entry(ip.clone()).or_default();
        entry.push_front(HistoryEntry::Status(HostStatus::Up));
        entry.push_front(HistoryEntry::Status(HostStatus::Down));
        entry.push_front(HistoryEntry::Status(HostStatus::Up));

        assert_eq!(history.get(&ip).unwrap().len(), 3);
        assert_eq!(
            *history.get(&ip).unwrap().front().unwrap(),
            HistoryEntry::Status(HostStatus::Up)
        );
    }

    #[test]
    fn test_history_max_size() {
        let mut history: HostHistory = HashMap::new();
        let ip = "192.168.1.1".to_string();

        let entry = history.entry(ip.clone()).or_default();

        // Add more than 200 items
        for i in 0..250 {
            entry.push_front(HistoryEntry::Status(if i % 2 == 0 {
                HostStatus::Up
            } else {
                HostStatus::Down
            }));

            // Keep only 200 (simulating the main loop logic)
            if entry.len() > 200 {
                entry.pop_back();
            }
        }

        assert_eq!(history.get(&ip).unwrap().len(), 200);
    }

    #[test]
    fn test_time_marker_in_history() {
        let mut history: HostHistory = HashMap::new();
        let ip = "192.168.1.1".to_string();

        let entry = history.entry(ip.clone()).or_default();
        entry.push_front(HistoryEntry::Status(HostStatus::Up));
        entry.push_front(HistoryEntry::TimeMarker);
        entry.push_front(HistoryEntry::Status(HostStatus::Down));

        assert_eq!(history.get(&ip).unwrap().len(), 3);
        assert_eq!(
            *history.get(&ip).unwrap().front().unwrap(),
            HistoryEntry::Status(HostStatus::Down)
        );
        assert_eq!(history.get(&ip).unwrap()[1], HistoryEntry::TimeMarker);
    }

    #[test]
    fn test_parse_real_world_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Network Hosts").unwrap();
        writeln!(file, "router,192.168.1.1").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "# Servers").unwrap();
        writeln!(file, "web-server,10.0.1.100").unwrap();
        writeln!(file, "db-server,10.0.1.200").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "# DNS").unwrap();
        writeln!(file, "dns1,8.8.8.8").unwrap();
        file.flush().unwrap();

        let result = parse_hosts_file(&file.path().to_path_buf());
        assert!(result.is_ok());

        let hosts = result.unwrap();
        assert_eq!(hosts.len(), 4);
        assert_eq!(hosts[0].name, "router");
        assert_eq!(hosts[1].name, "web-server");
        assert_eq!(hosts[2].name, "db-server");
        assert_eq!(hosts[3].name, "dns1");
    }
}
