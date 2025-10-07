# OxMon - Oxide Network Monitoring

A simple ping monitoring tool that tracks host availability and provides
both a web dashboard and CLI interface.

## Features

- **Continuous Monitoring**: Pings hosts every 15 seconds (single ping with 10s timeout)
- **Web Dashboard**: Real-time web interface showing host status
- **CLI Interface**: Query host status from the command line with timeline display
- **Database**: SQLite database tracking ping results and state transitions
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
cargo run --bin oxmon-server -- -f hosts.txt
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
# List all hosts with timeline (default: last 20 pings)
cargo run --bin oxmon-cli

# Show more history (last 50 pings)
cargo run --bin oxmon-cli -- -b 50

# Query a different server
cargo run --bin oxmon-cli -- --server-url http://localhost:8082
```

## Server Options

- `-b, --bind-address <ADDR>`: Bind address (default: 127.0.0.1)
- `-p, --bind-port <PORT>`: Bind port (default: 8082)
- `-f, --hosts-file <PATH>`: Path to hosts file (default: hosts.txt)
- `-d, --db-path <PATH>`: SQLite database path (default: oxmon.db)

## CLI Options

- `-s, --server-url <URL>`: Server URL (default: http://127.0.0.1:8082)
- `-b, --num-buckets <N>`: Number of ping results to display (default: 20)
- `-w, --width <N>`: Terminal width (auto-detect if not specified)

## API Endpoints

- `GET /`: Web dashboard
- `GET /api/hosts`: JSON list of all hosts and their current status
- `GET /api/timelines?num_buckets=20`: JSON list of hosts with timeline data

## Database Schema

The database tracks:
- **hosts**: Host configuration (hostname, IP, created_at)
- **host_events**: State transitions (online/offline/unknown) with timestamps
- **ping_results**: Individual ping results (responded boolean, timestamp)
- **server_sessions**: Server uptime tracking (started_at, stopped_at, shutdown_type)

## Timeline Display

The CLI shows a visual timeline of the most recent ping results:
- `W` = Host responded (online)
- `z` = Host did not respond (offline)
- `·` = No data (reserved, not currently used)

Timeline displays newest pings on the left, older pings on the right.

## TODO

### Timeline Display Bug
The timeline display has a critical bug: it does not include timestamps for the data being displayed, and does not fill in space to indicate the amount of time between each bucket. This means:
- Adjacent buckets appear equally spaced regardless of actual time gaps
- If the server was down for hours between pings, the display shows them side-by-side
- No way to tell when each ping occurred or how much time elapsed between pings
- The visual representation is misleading about temporal relationships

The display should either:
1. Include timestamps or time labels for the buckets, or
2. Add spacing/indicators to show temporal gaps between pings, or
3. Only show buckets from a continuous monitoring period without gaps

### Historical Analysis
The program currently does not display historical buckets beyond the most recent N pings. While ping results are stored in the database with timestamps, the system does not provide:
- Views of historical data from arbitrary time periods
- Time-based aggregation or bucketing
- Historical uptime statistics or reports
- Graphs showing availability over longer periods

Without these features, keeping extensive historical data in the database file provides limited value. Future work should either:
1. Implement historical analysis and reporting features, or
2. Add database cleanup/rotation to limit storage of ping results to recent history only

The `host_events` table and `server_sessions` tracking were designed for more sophisticated timeline analysis that is not currently implemented.
