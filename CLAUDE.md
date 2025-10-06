# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Git Workflow

**IMPORTANT:** When committing changes:
- NEVER use `git add -A` or `git add .`
- ALWAYS specify individual files explicitly: `git add file1 file2 file3`
- This prevents accidentally committing untracked files or temporary files

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
