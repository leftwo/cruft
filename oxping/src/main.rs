use clap::Parser;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    writeln!(
        stdout,
        "╔══════════════════════════════════════════════════╗"
    )?;
    writeln!(
        stdout,
        "║              OxPing Monitor                      ║"
    )?;
    writeln!(
        stdout,
        "╠══════════════════════════════════════════════════╣"
    )?;
    writeln!(
        stdout,
        "║ Host                    IP               Status  ║"
    )?;
    writeln!(
        stdout,
        "╠══════════════════════════════════════════════════╣"
    )?;

    for result in results {
        let status_char = match result.status {
            HostStatus::Up => "●",
            HostStatus::Down => "○",
        };
        let status_color = match result.status {
            HostStatus::Up => "\x1b[32m",   // green
            HostStatus::Down => "\x1b[31m", // red
        };
        writeln!(
            stdout,
            "║ {:<23} {:<16} {}{}\x1b[0m     ║",
            result.host.name, result.host.ip, status_color, status_char
        )?;
    }

    writeln!(
        stdout,
        "╚══════════════════════════════════════════════════╝"
    )?;
    writeln!(stdout, "\nPress Ctrl-C to exit")?;

    stdout.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    setup_panic_hook();

    let args = Args::parse();

    // Set up Ctrl-C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

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

    while running.load(Ordering::SeqCst) {
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

        // Wait for next interval or quit signal
        let elapsed = start.elapsed();
        if elapsed < Duration::from_secs(15) {
            tokio::time::sleep(Duration::from_secs(15) - elapsed).await;
        }
    }
}
