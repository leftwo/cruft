# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project: OxMon (Network Monitoring)

**Location:** `.` current directory

OxMon is a network monitoring tool that pings hosts and tracks their status over time. It provides a web dashboard and CLI for viewing host status.

### Architecture

The project is structured as a Rust workspace with five crates:

- **oxmon-common** - Shared types and data structures
  - HostConfig, HostStatus, PingResult
  - Status enum (Online/Offline)
  - EventType enum (Online/Offline/Unknown)
  - ServerSession tracking

- **oxmon-db** - SQLite database layer
  - hosts table (with first_connected timestamp)
  - host_events table (status transitions and monitoring gaps)
  - ping_results table (detailed ping data)
  - server_sessions table (tracks server uptime/downtime)

- **oxmon-core** - Core monitoring logic
  - Ping implementation (3 pings, 5s timeout)
  - Monitor that coordinates pinging and database updates
  - Handles server restart detection and gap recording
  - Config parser for hosts file (hostname,ip_address CSV format)

- **oxmon-server** - HTTP server and web dashboard
  - Dropshot REST API
  - HTML dashboard with auto-refresh
  - Simple status display (colored circles for on/off)

- **oxmon-cli** - Command-line interface
  - Query server for host status
  - Display simple table: hostname, IP, on/off

### Commands

All commands should be run from the `oxmon/` directory:

```bash
# Standard development workflow
cargo fmt && cargo clippy --all-targets && cargo test

# Run the server (first time with hosts file)
cargo run --bin oxmon-server -- -f hosts.txt

# Run the server (using existing database)
cargo run --bin oxmon-server

# Run the CLI
cargo run --bin oxmon-cli
```

### Server Details

- **Default address:** `127.0.0.1:8082`
- **Web dashboard:** `http://127.0.0.1:8082/`
- **API endpoint:** `GET /api/hosts` - Returns JSON list of all hosts
- **Ping interval:** 10 seconds (3 pings per host, 5 second timeout each)
- **Database:** SQLite file (default: `oxmon.db`)

### Key Features

**Server Session Tracking:**
- Tracks when server starts and stops
- Detects crashes (unclosed sessions)
- Records "unknown" events for all hosts on restart (monitoring gap)
- Distinguishes "host was down" from "we don't know (server was down)"

**Host History:**
- `first_connected` timestamp: when host first responded
- Complete event history: online → offline → [GAP] → unknown → online
- All ping results stored with timestamps

**Database Persistence:**
- First run: Load hosts from file → store in database
- Subsequent runs: Load hosts from database (optional hosts file to add/update)
- All runtime data persists across restarts

### Hosts File Format

CSV format with comments:
```
# Lines starting with # are comments
hostname,ip_address
server1,192.168.1.10
server2,10.0.0.5
```
