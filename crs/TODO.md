# CRS TODO

## Core Functionality Status

- [x] Client Implementation (registration, heartbeats, auto-detection)
- [x] Server Implementation (registry, API endpoints, web dashboard)

## Missing/Incomplete Features

### Configuration Files
- [ ] Server: Add config file support (port, thresholds, etc. are hardcoded)
- [ ] Client: Add config file support (only CLI args currently)

### Persistence
- [ ] Server registry is in-memory only (all data lost on restart)
- [ ] Add database or file-based storage option

### Client Deregistration
- [ ] Add explicit deregister endpoint
- [ ] Implement graceful shutdown handling (Ctrl+C should notify server)

### Reconnection Logic
- [ ] Client should re-register if server restarts
- [ ] Implement exponential backoff on failures

### Logging
- [ ] Add structured logging to server
- [ ] Replace client println!/eprintln! with proper logging

### Testing
- [ ] Add unit tests
- [ ] Add integration tests

### Security
- [ ] Add authentication/authorization
- [ ] Add TLS/HTTPS support
- [ ] Server should validate client registrations

### Observability
- [ ] Add metrics/telemetry
- [ ] Implement structured logging
- [ ] Add health check endpoint

### Client Features
- [ ] Allow adding custom tags via CLI
- [ ] Support updating client info without restart

### Server Features
- [ ] Add ability to manually remove/ban clients
- [ ] Implement pagination for large client lists
- [ ] Add filtering/search in web dashboard
- [ ] Add filtering/search in API endpoints
