use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
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

fn draw_ui(results: &[HostResult]) -> io::Result<()> {
    let mut stdout = io::stdout();

    // Get terminal size
    let (width, _height) = terminal::size()?;
    let width = width as usize;

    // Ensure minimum width
    let width = width.max(40);

    // Calculate column widths
    // Format: "║ Host IP Status ║"
    // Borders take 4 chars: "║ " and " ║"
    // Status takes 8 chars: " Status " (including space and symbol)
    let available = width.saturating_sub(4 + 8);
    let host_width = (available / 2).max(8);
    let ip_width = available.saturating_sub(host_width).max(7);

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
    let header = format!(
        "║ {:<host_width$} {:<ip_width$} Status ║",
        "Host",
        "IP",
        host_width = host_width,
        ip_width = ip_width
    );
    write!(stdout, "{}\r\n", header)?;

    // Draw separator
    write!(stdout, "{}\r\n", separator)?;

    // Draw each host
    for result in results {
        let status_char = match result.status {
            HostStatus::Up => "●",
            HostStatus::Down => "○",
        };
        let status_color = match result.status {
            HostStatus::Up => "\x1b[32m",   // green
            HostStatus::Down => "\x1b[31m", // red
        };

        // Truncate name and IP if needed
        let name = if result.host.name.len() > host_width {
            format!("{}…", &result.host.name[..host_width - 1])
        } else {
            result.host.name.clone()
        };

        let ip = if result.host.ip.len() > ip_width {
            format!("{}…", &result.host.ip[..ip_width - 1])
        } else {
            result.host.ip.clone()
        };

        write!(
            stdout,
            "║ {:<host_width$} {:<ip_width$} {}{}\x1b[0m      ║\r\n",
            name,
            ip,
            status_color,
            status_char,
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

        // Draw the UI
        if let Err(e) = draw_ui(&results) {
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

        // Use select to wait for either timeout or Ctrl-C
        tokio::select! {
            _ = tokio::time::sleep(sleep_duration) => {
                // Timeout reached, continue to next iteration
            }
            ctrl_c = tokio::task::spawn_blocking(move || {
                // Poll for keyboard events with short timeout
                loop {
                    if event::poll(Duration::from_millis(100)).unwrap_or(false)
                        && let Ok(Event::Key(key_event)) = event::read()
                            && key_event.code == KeyCode::Char('c')
                                && key_event.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                return true;
                            }
                }
            }) => {
                if ctrl_c.unwrap_or(false) {
                    break;
                }
            }
        }
    }
}
