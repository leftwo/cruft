use chrono::Local;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute, terminal,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Configuration constants
const PING_INTERVAL_SECS: u64 = 10;
const TIME_MARKER_INTERVAL_SECS: u64 = 60;
const MAX_HISTORY_SIZE: usize = 5000;

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

// Helper function to get visible entries from history with scroll offset
fn get_visible_entries(
    history: &[HistoryEntry],
    scroll_offset: usize,
    width: usize,
) -> Vec<HistoryEntry> {
    history
        .iter()
        .skip(scroll_offset)
        .take(width)
        .copied()
        .collect()
}

fn draw_ui(
    hosts: &[Host],
    history: &HostHistory,
    last_update: &str,
    scroll_offset: usize,
    previous_line_count: &mut usize,
) -> io::Result<()> {
    let mut stdout = io::stdout();

    // Get terminal size
    let (width, _height) = terminal::size()?;
    let width = width as usize;

    // Ensure minimum width
    let width = width.max(40);

    // Calculate column widths
    // Find longest hostname (max 16 chars) from hosts list
    let max_host_len = hosts
        .iter()
        .map(|h| h.name.len().min(16))
        .max()
        .unwrap_or(4);
    let host_width = max_host_len.max(4); // At least "Host"

    // IP is fixed at 15 chars (for IPv4 xxx.xxx.xxx.xxx)
    let ip_width = 15;

    // Timeline gets the rest, minus 2 for scroll indicators
    // Format: "Host IP < Timeline >"
    // spaces: 2 (1 between cols, 1 between cols) + 2 (left/right indicators)
    let timeline_width = width.saturating_sub(host_width + ip_width + 2 + 2);

    let mut current_line_count = 0;

    // Draw header with timestamp or mode indicator on the right
    let mode_indicator = if scroll_offset == 0 {
        format!("Updated: {}", last_update)
    } else {
        format!("PAUSED - Offset: {}", scroll_offset)
    };
    let header_left = format!(
        "{:<host_width$} {:<ip_width$}",
        "Host",
        "IP",
        host_width = host_width,
        ip_width = ip_width
    );
    let spacing = width
        .saturating_sub(header_left.len())
        .saturating_sub(mode_indicator.len());

    // Position cursor and write line padded to full width
    let header_line = format!("{}{}{}", header_left, " ".repeat(spacing), mode_indicator);
    let header_padded = format!("{:<width$}", header_line, width = width);
    execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
    write!(stdout, "{}\r\n", header_padded)?;
    current_line_count += 1;

    // Draw separator padded to full width
    let separator_line = format!("{:<width$}", "═".repeat(width), width = width);
    execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
    write!(stdout, "{}\r\n", separator_line)?;
    current_line_count += 1;

    // Draw each host with timeline (use hosts list so we show all hosts even without results)
    for host in hosts {
        // Truncate hostname to 16 chars max
        let name = if host.name.len() > 16 {
            &host.name[..16]
        } else {
            &host.name
        };

        // Get history for this host
        let host_history = history
            .get(&host.ip)
            .map(|h| h.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        // Build timeline with colored status characters and time markers
        // Skip scroll_offset entries, then take timeline_width entries
        let mut timeline = String::new();
        let visible_entries = get_visible_entries(&host_history, scroll_offset, timeline_width);

        for entry in &visible_entries {
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
        let blocks_shown = visible_entries.len();
        if blocks_shown < timeline_width {
            timeline.push_str(&" ".repeat(timeline_width - blocks_shown));
        }

        // Determine scroll indicators
        let has_older = scroll_offset + timeline_width < host_history.len();
        let has_newer = scroll_offset > 0;
        let left_indicator = if has_newer { "<" } else { " " };
        let right_indicator = if has_older { ">" } else { " " };

        // Build the complete line and pad to full width
        let host_line = format!(
            "{:<host_width$} {:<ip_width$} {}{}{}",
            name,
            host.ip,
            left_indicator,
            timeline,
            right_indicator,
            host_width = host_width,
            ip_width = ip_width
        );
        let host_line_padded = format!("{:<width$}", host_line, width = width);
        execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
        write!(stdout, "{}\r\n", host_line_padded)?;
        current_line_count += 1;
    }

    // Blank line padded to full width
    let blank_line = format!("{:<width$}", "", width = width);
    execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
    write!(stdout, "{}\r\n", blank_line)?;
    current_line_count += 1;

    // Footer line padded to full width
    let footer_line = format!("{:<width$}", "Ctrl-C to exit, left/right arrows to see history", width = width);
    execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
    write!(stdout, "{}", footer_line)?;
    current_line_count += 1;

    // Overwrite any remaining lines from previous display with blank lines
    while current_line_count < *previous_line_count {
        let blank_line = format!("{:<width$}", "", width = width);
        execute!(stdout, cursor::MoveTo(0, current_line_count as u16))?;
        write!(stdout, "{}", blank_line)?;
        current_line_count += 1;
    }

    *previous_line_count = current_line_count;

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

    // Shared state between ping task and UI task
    let history: Arc<Mutex<HostHistory>> = Arc::new(Mutex::new(HashMap::new()));
    let results: Arc<Mutex<Vec<HostResult>>> = Arc::new(Mutex::new(Vec::new()));

    // Clone for ping task
    let history_clone = Arc::clone(&history);
    let results_clone = Arc::clone(&results);
    let hosts_clone = hosts.clone();

    // Spawn background ping task
    tokio::spawn(async move {
        let iterations_per_marker = TIME_MARKER_INTERVAL_SECS / PING_INTERVAL_SECS;
        let mut iteration_count = 0;

        loop {
            // Ping all hosts in parallel
            let mut tasks = Vec::new();
            for host in &hosts_clone {
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
            let mut ping_results = Vec::new();
            for task in tasks {
                match task.await {
                    Ok(Ok(result)) => ping_results.push(result),
                    Ok(Err((host, _err))) => {
                        ping_results.push(HostResult {
                            host,
                            status: HostStatus::Down,
                        });
                    }
                    Err(_) => {}
                }
            }

            // Update shared results and history
            {
                let mut res = results_clone.lock().unwrap();
                *res = ping_results.clone();
            }

            {
                let mut hist = history_clone.lock().unwrap();
                for result in &ping_results {
                    let entry = hist.entry(result.host.ip.clone()).or_default();
                    entry.push_front(HistoryEntry::Status(result.status));
                    if entry.len() > MAX_HISTORY_SIZE {
                        entry.pop_back();
                    }
                }
            }

            iteration_count += 1;

            // Add time marker at the calculated interval
            if iteration_count > 0 && iteration_count % iterations_per_marker == 0 {
                let mut hist = history_clone.lock().unwrap();
                for host in &hosts_clone {
                    let entry = hist.entry(host.ip.clone()).or_default();
                    entry.push_front(HistoryEntry::TimeMarker);
                    if entry.len() > MAX_HISTORY_SIZE {
                        entry.pop_back();
                    }
                }
            }

            // Wait for next interval
            tokio::time::sleep(Duration::from_secs(PING_INTERVAL_SECS)).await;
        }
    });

    // UI task (main thread)
    let mut scroll_offset: usize = 0;
    let mut previous_line_count: usize = 0;
    let mut is_first_update = true;

    loop {
        // Get current state from shared data
        let current_history = {
            let hist = history.lock().unwrap();
            hist.clone()
        };

        // Format current time for display
        let last_update = Local::now().format("%H:%M:%S").to_string();

        // Draw the UI
        if let Err(e) = draw_ui(
            &hosts,
            &current_history,
            &last_update,
            scroll_offset,
            &mut previous_line_count,
        ) {
            eprintln!("Error drawing UI: {}", e);
            break;
        }

        // Different behavior based on mode
        let mut should_exit = false;

        if scroll_offset == 0 {
            // Live mode: wait for timer interval and check for keys
            // First update waits 5 seconds, subsequent updates wait 10 seconds
            let sleep_duration = if is_first_update {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(PING_INTERVAL_SECS)
            };

            let start_sleep = tokio::time::Instant::now();
            while start_sleep.elapsed() < sleep_duration {
                // Poll for keyboard event with short timeout
                let poll_result = tokio::task::spawn_blocking(|| {
                    if event::poll(Duration::from_millis(100)).unwrap_or(false)
                        && let Ok(evt) = event::read()
                    {
                        match evt {
                            Event::Key(key_event) => match key_event.code {
                                KeyCode::Char('c')
                                    if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    return Some(KeyCode::Char('c'));
                                }
                                KeyCode::Left => return Some(KeyCode::Left),
                                KeyCode::Right => return Some(KeyCode::Right),
                                KeyCode::Enter => return Some(KeyCode::Enter),
                                _ => {}
                            },
                            Event::Resize(_, _) => {
                                return Some(KeyCode::F(1)); // Use F1 as resize signal
                            }
                            _ => {}
                        }
                    }
                    None
                })
                .await;

                if let Ok(Some(key)) = poll_result {
                    match key {
                        KeyCode::Char('c') => {
                            should_exit = true;
                            break;
                        }
                        KeyCode::Right => {
                            // Scroll backward (into history)
                            let max_history_len =
                                current_history.values().map(|h| h.len()).max().unwrap_or(0);
                            let terminal_width =
                                terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
                            let max_offset = max_history_len.saturating_sub(terminal_width / 2);
                            if scroll_offset < max_offset {
                                scroll_offset = scroll_offset.saturating_add(1);
                                break; // Exit sleep loop to refresh display immediately
                            }
                        }
                        KeyCode::F(1) => {
                            // Window resize detected - clear screen and redraw
                            let _ = execute!(
                                io::stdout(),
                                terminal::Clear(terminal::ClearType::All),
                                cursor::MoveTo(0, 0)
                            );
                            previous_line_count = 0;
                            break; // Exit sleep loop to refresh display immediately
                        }
                        _ => {}
                    }
                }
            }

            // Mark first update as complete
            if is_first_update {
                is_first_update = false;
            }
        } else {
            // History mode: block waiting for keypress only
            let poll_result = tokio::task::spawn_blocking(|| {
                // Block indefinitely waiting for a key
                if let Ok(evt) = event::read() {
                    match evt {
                        Event::Key(key_event) => match key_event.code {
                            KeyCode::Char('c')
                                if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                return Some(KeyCode::Char('c'));
                            }
                            KeyCode::Left => return Some(KeyCode::Left),
                            KeyCode::Right => return Some(KeyCode::Right),
                            KeyCode::Enter => return Some(KeyCode::Enter),
                            _ => {}
                        },
                        Event::Resize(_, _) => {
                            return Some(KeyCode::F(1)); // Use F1 as resize signal
                        }
                        _ => {}
                    }
                }
                None
            })
            .await;

            if let Ok(Some(key)) = poll_result {
                match key {
                    KeyCode::Char('c') => {
                        should_exit = true;
                    }
                    KeyCode::Left => {
                        // Scroll forward (toward present)
                        if scroll_offset > 0 {
                            scroll_offset = scroll_offset.saturating_sub(1);
                        }
                    }
                    KeyCode::Right => {
                        // Scroll backward (into history)
                        let max_history_len =
                            current_history.values().map(|h| h.len()).max().unwrap_or(0);
                        let terminal_width =
                            terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
                        let max_offset = max_history_len.saturating_sub(terminal_width / 2);
                        if scroll_offset < max_offset {
                            scroll_offset = scroll_offset.saturating_add(1);
                        }
                    }
                    KeyCode::Enter => {
                        // Return to live mode
                        scroll_offset = 0;
                    }
                    KeyCode::F(1) => {
                        // Window resize detected - clear screen and redraw
                        let _ = execute!(
                            io::stdout(),
                            terminal::Clear(terminal::ClearType::All),
                            cursor::MoveTo(0, 0)
                        );
                        previous_line_count = 0;
                    }
                    _ => {}
                }
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

        // Add more than MAX_HISTORY_SIZE items
        for i in 0..(MAX_HISTORY_SIZE + 50) {
            entry.push_front(HistoryEntry::Status(if i % 2 == 0 {
                HostStatus::Up
            } else {
                HostStatus::Down
            }));

            // Keep only MAX_HISTORY_SIZE (simulating the main loop logic)
            if entry.len() > MAX_HISTORY_SIZE {
                entry.pop_back();
            }
        }

        assert_eq!(history.get(&ip).unwrap().len(), MAX_HISTORY_SIZE);
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

    #[test]
    fn test_scrolling_timeline_display() {
        // Create a history with known data pattern
        let mut history: VecDeque<HistoryEntry> = VecDeque::new();

        // Push oldest to newest (last push_front = position 0)
        history.push_front(HistoryEntry::Status(HostStatus::Down)); // oldest (position 9)
        history.push_front(HistoryEntry::Status(HostStatus::Down)); // position 8
        history.push_front(HistoryEntry::Status(HostStatus::Down)); // position 7
        history.push_front(HistoryEntry::TimeMarker); // position 6
        history.push_front(HistoryEntry::Status(HostStatus::Up)); // position 5
        history.push_front(HistoryEntry::Status(HostStatus::Up)); // position 4
        history.push_front(HistoryEntry::Status(HostStatus::Up)); // position 3
        history.push_front(HistoryEntry::TimeMarker); // position 2
        history.push_front(HistoryEntry::Status(HostStatus::Down)); // position 1
        history.push_front(HistoryEntry::Status(HostStatus::Down)); // position 0 (newest)

        // Convert to Vec as the function expects
        let history_vec: Vec<HistoryEntry> = history.iter().copied().collect();

        // Verify what we actually have
        assert_eq!(history_vec.len(), 10);
        // Position 0 should be newest (last push_front)
        assert_eq!(history_vec[0], HistoryEntry::Status(HostStatus::Down));
        assert_eq!(history_vec[1], HistoryEntry::Status(HostStatus::Down));
        assert_eq!(history_vec[2], HistoryEntry::TimeMarker);

        // Test with scroll_offset = 0 (live mode, show newest 5 entries)
        let visible = get_visible_entries(&history_vec, 0, 5);
        assert_eq!(visible.len(), 5);
        assert_eq!(visible[0], HistoryEntry::Status(HostStatus::Down)); // newest
        assert_eq!(visible[1], HistoryEntry::Status(HostStatus::Down));
        assert_eq!(visible[2], HistoryEntry::TimeMarker);
        assert_eq!(visible[3], HistoryEntry::Status(HostStatus::Up));
        assert_eq!(visible[4], HistoryEntry::Status(HostStatus::Up));

        // Test with scroll_offset = 3 (scrolled back 3 positions)
        let visible = get_visible_entries(&history_vec, 3, 5);
        assert_eq!(visible.len(), 5);
        assert_eq!(visible[0], HistoryEntry::Status(HostStatus::Up)); // now showing older data
        assert_eq!(visible[1], HistoryEntry::Status(HostStatus::Up));
        assert_eq!(visible[2], HistoryEntry::Status(HostStatus::Up));
        assert_eq!(visible[3], HistoryEntry::TimeMarker);
        assert_eq!(visible[4], HistoryEntry::Status(HostStatus::Down));

        // Test with scroll_offset = 7 (scrolled back to oldest data)
        let visible = get_visible_entries(&history_vec, 7, 5);
        assert_eq!(visible.len(), 3); // Only 3 entries left
        assert_eq!(visible[0], HistoryEntry::Status(HostStatus::Down));
        assert_eq!(visible[1], HistoryEntry::Status(HostStatus::Down));
        assert_eq!(visible[2], HistoryEntry::Status(HostStatus::Down));

        // Test with scroll_offset beyond available data
        let visible = get_visible_entries(&history_vec, 20, 5);
        assert_eq!(visible.len(), 0); // No data available
    }

    #[test]
    fn test_scrolling_with_time_markers() {
        // Create history with alternating statuses and time markers
        let mut history: VecDeque<HistoryEntry> = VecDeque::new();

        for i in 0..10 {
            if i % 3 == 0 {
                history.push_front(HistoryEntry::TimeMarker);
            } else if i % 2 == 0 {
                history.push_front(HistoryEntry::Status(HostStatus::Up));
            } else {
                history.push_front(HistoryEntry::Status(HostStatus::Down));
            }
        }

        let history_vec: Vec<HistoryEntry> = history.iter().copied().collect();

        // Verify we can scroll and see time markers at different positions
        let visible_0 = get_visible_entries(&history_vec, 0, 3);
        let visible_3 = get_visible_entries(&history_vec, 3, 3);
        let visible_6 = get_visible_entries(&history_vec, 6, 3);

        // All should have different data
        assert_ne!(visible_0, visible_3);
        assert_ne!(visible_3, visible_6);

        // Verify time markers appear in expected positions
        let all_visible = get_visible_entries(&history_vec, 0, 10);
        let marker_count = all_visible
            .iter()
            .filter(|e| **e == HistoryEntry::TimeMarker)
            .count();
        assert_eq!(marker_count, 4); // 0, 3, 6, 9 = 4 markers
    }
}
