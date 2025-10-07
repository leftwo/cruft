# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## General Development Guidelines

### Git Workflow

**IMPORTANT:** When committing changes:
- NEVER use `git add -A` or `git add .`
- ALWAYS specify individual files explicitly: `git add file1 file2 file3`
- This prevents accidentally committing untracked files or temporary files
- After making changes, propose files to add and commit message, wait for user approval
- Do NOT include emoji or tool references (like "🤖 Generated with Claude Code") in commit messages
- Keep git commit lines to 80 characters wide, but take as many lines as needed

### Rust Development

**Standard Workflow:**
```bash
# After making changes, always run:
cargo fmt && cargo clippy --all-targets && cargo test
```

**Code Quality:**
- Use Rust edition 2024
- Honor rustfmt settings (max_width = 80)
- ALWAYS fix clippy warnings - do not ignore them unless specifically instructed
- Do NOT add `-D warnings` flag to clippy commands
- Use `cargo check` for quick validation instead of `cargo build --release`

**Project Structure:**
- Keep crates at top level, not in subdirectories

## Repository Overview

This repository contains multiple independent programs. Each program lives in its own subdirectory.

## Project: CRS (Central Registry Service)

**Location:** `crs/` directory

The CRS is a client/server system where clients register with a central service to advertise their presence and status. The server provides a web interface to view all connected clients in real-time.

### Architecture

The project is structured as a Rust workspace with three crates:

- **crs-common** (`crates/crs-common/`) - Shared protocol definitions and types
  - Protocol message types (RegisterRequest, HeartbeatRequest, etc.)
  - ClientId generation (deterministic UUID v5 from hostname+OS+IP)
  - Common data structures used by both client and server

- **crs-server** (`crates/crs-server/`) - Central registry service
  - In-memory client registry with status tracking
  - REST API endpoints via Dropshot framework
  - Web dashboard showing all registered clients
  - Background task for status updates (online/stale/offline)

- **crs-client** (`crates/crs-client/`) - Client library and agent
  - Library for connecting to CRS server
  - Example client implementation

### Commands

All commands should be run from the `crs/` directory:

```bash
# Build all crates
cargo build

# Build release version
cargo build --release

# Run the server
cargo run --bin crs-server

# Run the client
cargo run --bin crs-client

# Run tests
cargo test

# Check code without building
cargo check

# Format code (max_width = 80)
cargo fmt
```

### Server Details

- **Default address:** `127.0.0.1:8081`
- **Web dashboard:** `http://127.0.0.1:8081/`
- **API endpoints:**
  - `POST /api/register` - Register a new client
  - `POST /api/heartbeat` - Send heartbeat
  - `GET /api/clients` - List all registered clients

### Client Status Thresholds

- **Online:** Last heartbeat < 60 seconds ago
- **Stale:** Last heartbeat 60-180 seconds ago
- **Offline:** Last heartbeat > 180 seconds ago

Status updates happen automatically every 30 seconds via background task.

### Client ID Generation

Client IDs are deterministic UUIDs (v5) generated from:
- Hostname
- Operating system

This ensures the same client gets the same ID across restarts. The server
determines the client's IP address from the connection.

### Code Organization

- `main.rs` - Server initialization and startup
- `registry.rs` - Client storage and status management
- `api.rs` - REST API endpoint handlers (Dropshot)
- `web.rs` - HTML dashboard generation

## Project: OxMon (Network Monitoring)

**Location:** `oxmon/` directory

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
