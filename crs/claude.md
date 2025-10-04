# CRS (Central Registry Service) - Development Notes

## Project Overview

CRS is a centralized service for tracking client connections and status. It consists of a server that maintains a registry of clients, and clients that register themselves and send periodic heartbeats.

### Architecture

**Workspace Structure:**
- `crates/crs-common/` - Shared types and protocol definitions
- `crates/crs-server/` - Server implementation with REST API and web dashboard
- `crates/crs-client/` - Client library and binary

**Key Technologies:**
- Dropshot - REST API framework
- Tokio - Async runtime
- Chrono - Timestamp and duration handling
- Serde - Serialization/deserialization
- TOML - Configuration file format
- Reqwest - HTTP client

## Protocol & Status Management

### Client Status States
Clients transition through three states based on heartbeat timing:
- **Online**: Last heartbeat < 40 seconds ago (< 1 missed heartbeat)
- **Stale**: Last heartbeat 40-80 seconds ago (1-3 missed heartbeats)
- **Offline**: Last heartbeat > 80 seconds ago (4+ missed heartbeats)

### Timing Configuration
- **Heartbeat Interval**: 20 seconds
- **Status Update Interval**: Server checks every 30 seconds
- **Registration Retry**: 10 seconds (if initial connection fails)

### Client ID Generation
Client IDs are deterministic UUIDs (v5) generated from `hostname:os`. This ensures:
- Same client gets same ID across restarts
- No need for server-side ID assignment
- Automatic client identification

## REST API Endpoints

### POST /api/register
Register a new client with server.

**Request:**
```json
{
  "client_info": {
    "hostname": "string",
    "os": "string",
    "ip_address": "string",
    "version": "string",
    "tags": {}
  }
}
```

**Response:**
```json
{
  "client_id": "uuid",
  "heartbeat_interval_secs": 20
}
```

**Note:** Server overrides `ip_address` with the actual connection IP.

### POST /api/heartbeat
Send heartbeat to maintain online status.

**Request:**
```json
{
  "client_id": "uuid"
}
```

**Response:**
```json
{
  "server_time": "2025-10-04T12:34:56Z"
}
```

### GET /api/clients
List all registered clients with their status.

**Response:**
```json
{
  "clients": [
    {
      "client_id": "uuid",
      "hostname": "string",
      "os": "string",
      "ip_address": "string",
      "version": "string",
      "tags": {},
      "status": "online|stale|offline",
      "registered_at": "2025-10-04T12:34:56Z",
      "last_heartbeat": "2025-10-04T12:34:56Z"
    }
  ],
  "server_start_time": "2025-10-04T12:00:00Z"
}
```

### GET /
Web dashboard showing server uptime and all registered clients in HTML format.

## Binaries

### crs-server
Main server binary that runs the REST API and web dashboard.

**Usage:**
```bash
crs-server --server-address 127.0.0.1 --port 8081
```

**Options:**
- `-s, --server-address <IP>` - IP address to bind to (default: 127.0.0.1)
- `-p, --port <PORT>` - Port to listen on (default: 8081)

**Features:**
- Serves REST API on `/api/*`
- Serves web dashboard on `/`
- Auto-refreshing web dashboard (10 second refresh)
- Background status update task (30 second interval)
- Server uptime tracking

### crs-client
Client binary that registers with server and sends heartbeats.

**Usage:**
```bash
# Using CLI arguments
crs-client --server http://127.0.0.1:8081

# Using config file
crs-client --config client-config.toml

# CLI overrides config file (with warning)
crs-client --server http://other:8081 --config client-config.toml
```

**Options:**
- `-s, --server <URL>` - URL of the CRS server
- `-c, --config <PATH>` - Path to TOML configuration file

**Config File Format (client-config.toml):**
```toml
server = "http://127.0.0.1:8081"
```

**Features:**
- Auto-detects hostname, OS, IP address, version
- Supports custom tags via HashMap
- Automatic heartbeat loop (20 second interval)
- Retry logic on initial connection failure (10 second retry)
- Graceful shutdown on Ctrl+C

### crs-check
Command-line status viewer that fetches and displays server/client status.

**Usage:**
```bash
# Using CLI arguments
crs-check --server http://127.0.0.1:8081

# Using config file
crs-check --config check-config.toml
```

**Options:**
- `-s, --server <URL>` - URL of the CRS server
- `-c, --config <PATH>` - Path to TOML configuration file

**Config File Format (check-config.toml):**
```toml
server = "http://127.0.0.1:8081"
```

**Output Format:**
- 80-column text display
- Server uptime (no other server details)
- Table of registered clients with:
  - Hostname (16 chars)
  - IP Address (15 chars)
  - OS (7 chars)
  - Version (8 chars)
  - Status (8 chars)
  - Time Connected (14 chars)
- Strings truncated with "..." if too long

## Configuration System

### Design Pattern
All binaries follow a consistent configuration pattern:
1. Define `Args` struct with clap for CLI arguments
2. Define `Config` struct with serde for TOML file
3. Implement `load_config()` to read TOML file
4. Implement `resolve_config()` to merge CLI and file config
5. CLI arguments override config file values (with warning)
6. Test to ensure CLI and Config fields stay in sync

### Test: CLI/Config Field Synchronization
Each binary with config support has a test `test_cli_and_config_fields_match()` that:
- Lists all CLI fields in a HashSet
- Lists all Config fields in a HashSet
- Asserts no CLI fields are missing from Config
- Asserts no extra Config fields not in CLI

This test will **fail** if someone adds a CLI option without adding it to the Config struct, preventing configuration drift.

## Key Implementation Details

### ApiContext
Server state passed to all endpoint handlers:
```rust
pub struct ApiContext {
    pub registry: Registry,
    pub start_time: chrono::DateTime<chrono::Utc>,
}
```

### Registry (Thread-Safe)
The registry uses `Arc<Mutex<HashMap>>` for thread-safe shared state:
```rust
pub struct Registry {
    clients: Arc<Mutex<HashMap<ClientId, ClientEntry>>>,
}
```

Can be cloned cheaply (Arc clone) for use in:
- Multiple API endpoints
- Background status update task
- Integration tests

### Duration Formatting Pattern
Human-readable duration formatting used consistently across web dashboard, crs-check, and tests:
```rust
if duration.num_days() > 0 {
    format!("{}d {}h", duration.num_days(), duration.num_hours() % 24)
} else if duration.num_hours() > 0 {
    format!("{}h {}m", duration.num_hours(), duration.num_minutes() % 60)
} else if duration.num_minutes() > 0 {
    format!("{}m", duration.num_minutes())
} else {
    format!("{}s", duration.num_seconds())
}
```

### Clippy Warning Suppression
Dropshot's `#[endpoint]` macro generates code that appears unused when checked per-compilation-unit:
- Added `#![allow(dead_code)]` at module level in `api.rs` and `web.rs`
- This suppresses false-positive warnings for endpoint functions
- Not applied crate-wide, only where specifically needed

## Testing

### Test Coverage
- **56 total tests** across all crates
- Unit tests for core functionality
- Integration tests for API endpoints
- Doc tests for public APIs

### Unit Tests
- `crates/crs-common/` - 10 tests (client ID generation, serialization)
- `crates/crs-server/src/registry.rs` - 7 tests (registration, heartbeat, status transitions)
- `crates/crs-server/src/bin/crs-check.rs` - 4 tests (formatting functions, CLI/config sync)
- `crates/crs-client/src/main.rs` - 3 tests (config parsing, CLI/config sync)

### Integration Tests
- `crates/crs-server/tests/integration_test.rs` - 11 tests (full API flow)
- `crates/crs-server/tests/check_integration_test.rs` - 5 tests (crs-check functionality)

**Integration Test Pattern:**
Tests use the Registry directly without starting an HTTP server:
```rust
let registry = Registry::new();
let start_time = Utc::now();
// Directly call registry methods
registry.register(client_info);
registry.heartbeat(client_id);
// Verify state changes
let clients = registry.list_clients();
```

This allows fast, reliable tests of state transitions and business logic.

## Development Workflow

After each change:
1. `cargo fmt` - Format code
2. `cargo check` - Check compilation
3. `cargo test` - Run all tests (must pass)
4. Propose files to `git add`
5. Propose commit message
6. User approves with "yes" or "proceed"
7. Commit without "Generated with Claude Code" or "Co-Authored-By" messages

## Change History

### Session 1 (Previous)
- Initial project creation
- 3-crate workspace setup
- Client/server implementation
- Web dashboard
- Comprehensive test suite

### Session 2 (Current)

#### Commit 1: Update web dashboard with Time Connected column
- Removed Status and Last Heartbeat from server info table
- Added "Time Connected" column to client table
- Shows human-readable duration since registration

#### Commit 2: Adjust heartbeat timing and status thresholds
- Changed heartbeat interval: 30s → 20s
- Changed status thresholds: 60s/180s → 40s/80s
- Stale after 1 missed heartbeat (40s)
- Offline after 4 missed heartbeats (80s)
- Changed initial connection retry: 5s → 10s
- Updated all tests and documentation

#### Commit 3: Add TOML configuration support to crs-client
- Added `--config` flag to read options from TOML file
- CLI arguments override config file (with warning)
- Created `Config` struct with same fields as CLI `Args`
- Added `test_cli_and_config_fields_match()` validation test
- Created `example-config.toml`

#### Commit 4: Add crs-check binary for CLI status viewing
- New binary `crs-check` in crs-server crate
- Fetches data from `/api/clients` endpoint
- 80-column formatted text output
- Same config pattern as crs-client (TOML + CLI)
- 4 unit tests (format helpers, CLI/config sync)
- 5 integration tests (status display, state changes)
- Created `example-check-config.toml`

#### Commit 5: Fix clippy dead_code warnings
- Added module-level `#![allow(dead_code)]` to api.rs and web.rs
- Suppresses false-positive warnings for Dropshot endpoint functions
- Not applied crate-wide, only where specifically needed
- Note: User questioned if this analysis is correct

#### Commit 6: Add server uptime display
- Added `start_time` field to `ApiContext`
- Added `server_start_time` to `ListClientsResponse`
- Updated crs-check to show only server uptime (not full server details)
- Updated web dashboard to display server uptime
- Human-readable uptime formatting (days/hours, hours/minutes, etc.)

#### Commit 7: Remove unused server_url parameter
- Removed `server_url` parameter from `display_status()` in crs-check
- Parameter was no longer needed after simplifying display

## Known Issues & Future Work

See `TODO.md` for comprehensive list. Key items:

### Completed (from TODO.md)
- ✅ Client: Add config file support (completed in Session 2)
- ✅ Testing: Add unit tests (comprehensive coverage)
- ✅ Testing: Add integration tests (11 integration tests)

### Still Missing
- Server: Add config file support (port, thresholds are hardcoded)
- Persistence: Registry is in-memory only
- Client deregistration: No explicit deregister endpoint
- Reconnection: Client should detect server restarts
- Security: No authentication, TLS, or authorization
- Logging: Using println!/eprintln! instead of structured logging
- Observability: No metrics, telemetry, or health checks

## Design Principles

1. **Deterministic Client IDs** - Same client always gets same ID (UUID v5 from hostname:os)
2. **Thread-Safe Shared State** - Registry uses Arc<Mutex<HashMap>> for safe concurrent access
3. **Config Override Pattern** - CLI arguments override config file, with warning
4. **Test Validation** - Tests enforce CLI/Config field parity to prevent drift
5. **Human-Readable Output** - Duration formatting prioritizes readability (e.g., "2d 5h" not "177600s")
6. **Integration Testing Without HTTP** - Tests use Registry directly for faster, more reliable tests
7. **Consistent Formatting** - All binaries use same duration formatting pattern
8. **Module-Level Allow Attributes** - Only suppress warnings where specifically needed, not crate-wide

## Common Patterns

### Duration Formatting
See "Duration Formatting Pattern" above - used in web.rs, crs-check.rs, and tests.

### Config Resolution
```rust
fn resolve_config(args: Args) -> Result<ResolvedConfig> {
    let file_config = if let Some(config_path) = &args.config {
        Some(load_config(config_path)?)
    } else {
        None
    };

    // CLI overrides config file with warning
    let value = if let Some(cli_value) = args.value {
        if file_config.as_ref().and_then(|c| c.value.as_ref()).is_some() {
            eprintln!("Warning: Value specified in both config file and command line. Using command line value.");
        }
        cli_value
    } else if let Some(ref cfg) = file_config {
        cfg.value.clone().context("Value not specified in config file")?
    } else {
        anyhow::bail!("Value must be specified via CLI or config file");
    };

    Ok(ResolvedConfig { value })
}
```

### Status Update Background Task
```rust
let registry_clone = registry.clone();
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        registry_clone.update_statuses();
    }
});
```

## File Locations

### Core Implementation
- `crates/crs-common/src/lib.rs` - Protocol types and client ID generation
- `crates/crs-server/src/registry.rs` - Thread-safe client registry
- `crates/crs-server/src/api.rs` - REST API endpoint handlers
- `crates/crs-server/src/web.rs` - Web dashboard HTML generation
- `crates/crs-server/src/main.rs` - Server binary entry point
- `crates/crs-client/src/lib.rs` - Client library implementation
- `crates/crs-client/src/main.rs` - Client binary entry point
- `crates/crs-server/src/bin/crs-check.rs` - Status viewer binary

### Tests
- `crates/crs-server/tests/integration_test.rs` - API integration tests
- `crates/crs-server/tests/check_integration_test.rs` - crs-check integration tests

### Configuration Examples
- `crates/crs-client/example-config.toml` - Example client config
- `crates/crs-server/example-check-config.toml` - Example crs-check config

### Documentation
- `TODO.md` - Feature checklist and known issues
- `claude.md` - This file (development notes and change history)
