# OxMon - Oxide Network Monitoring

A ping monitoring tool that tracks host availability and provides
both a web dashboard and CLI interface.

## Features

- **Continuous Monitoring**: Pings hosts every 10 seconds (3 pings
  per check with 5s timeout)
- **Web Dashboard**: Real-time web interface showing host status
- **CLI Interface**: Query host status from the command line
- **Database**: SQLite database tracking state transitions for
  uptime analysis
- **Configurable**: Specify bind address, port, and hosts file

## Architecture

- **oxmon-common**: Shared types and data structures
- **oxmon-db**: SQLite database layer for persistence
- **oxmon-core**: Ping engine and monitoring logic
- **oxmon-server**: HTTP server with web dashboard and REST API
- **oxmon-cli**: Command-line tool for querying server

## Quick Start

### 1. Create a hosts file

Create `hosts.txt` with your hosts (format: `hostname,ip_address`):

```
google-dns,8.8.8.8
cloudflare-dns,1.1.1.1
localhost,127.0.0.1
```

### 2. Start the server

```bash
cargo run --bin oxmon-server
```

Or with custom options:

```bash
cargo run --bin oxmon-server -- \
    --bind-address 0.0.0.0 \
    --bind-port 8082 \
    --hosts-file hosts.txt \
    --db-path oxmon.db
```

### 3. View the dashboard

Open http://127.0.0.1:8082/ in your browser

### 4. Query from CLI

```bash
# List all hosts
cargo run --bin oxmon

# Query a different server
cargo run --bin oxmon -- --server-url http://localhost:8082 list
```

## Server Options

- `-b, --bind-address <ADDR>`: Bind address (default: 127.0.0.1)
- `-p, --bind-port <PORT>`: Bind port (default: 8082)
- `-f, --hosts-file <PATH>`: Path to hosts file (default:
  hosts.txt)
- `-d, --db-path <PATH>`: SQLite database path (default: oxmon.db)

## CLI Options

- `-s, --server-url <URL>`: Server URL (default:
  http://127.0.0.1:8082)

## API Endpoints

- `GET /`: Web dashboard
- `GET /api/hosts`: JSON list of all hosts and their status

## Database Schema

The database tracks:
- **hosts**: Host configuration (hostname, IP)
- **host_events**: State transitions (online/offline) for uptime
  tracking
- **ping_results**: Detailed ping statistics (optional)

## Future Features

- Uptime statistics and reporting
- Historical graphs and charts
- Host event timeline
- Alerting on state changes
