// Copyright 2025 Oxide Computer Company

//! REST API handlers
//!
//! This module contains the Dropshot endpoint handlers for the CRS REST API.
//! All endpoints use JSON for request and response bodies.

use crate::registry::Registry;
use chrono::Utc;
use crs_common::{
    HeartbeatRequest, HeartbeatResponse, ListClientsResponse, RegisterRequest,
    RegisterResponse,
};
use dropshot::{
    endpoint, HttpError, HttpResponseOk, RequestContext, TypedBody,
};

/// Context passed to all API handlers
///
/// Contains shared state that all endpoint handlers can access.
pub struct ApiContext {
    /// The client registry
    pub registry: Registry,
}

/// Register a new client
///
/// Accepts client information (hostname, OS, IP, version, tags) and
/// registers the client in the registry. Returns the client's deterministic
/// ID and the recommended heartbeat interval.
#[endpoint {
    method = POST,
    path = "/api/register",
}]
pub async fn register(
    ctx: RequestContext<ApiContext>,
    body: TypedBody<RegisterRequest>,
) -> Result<HttpResponseOk<RegisterResponse>, HttpError> {
    let request = body.into_inner();
    let registry = &ctx.context().registry;

    // Extract the client's actual IP address from the connection
    let client_ip = ctx.request.remote_addr().ip().to_string();

    // Override the IP address with what we actually see
    let mut client_info = request.client_info;
    client_info.ip_address = client_ip;

    let client_id = registry.register(client_info);

    Ok(HttpResponseOk(RegisterResponse {
        client_id,
        heartbeat_interval_secs: 30,
    }))
}

/// Record a client heartbeat
///
/// Updates the last heartbeat timestamp for a registered client.
/// Returns an error if the client ID is not found in the registry.
#[endpoint {
    method = POST,
    path = "/api/heartbeat",
}]
pub async fn heartbeat(
    ctx: RequestContext<ApiContext>,
    body: TypedBody<HeartbeatRequest>,
) -> Result<HttpResponseOk<HeartbeatResponse>, HttpError> {
    let request = body.into_inner();
    let registry = &ctx.context().registry;

    registry
        .heartbeat(request.client_id)
        .map_err(|e| HttpError::for_not_found(None, e.to_string()))?;

    Ok(HttpResponseOk(HeartbeatResponse {
        server_time: Utc::now(),
    }))
}

/// List all registered clients
///
/// Returns a list of all clients registered in the system with their
/// current status, registration time, and last heartbeat time.
#[endpoint {
    method = GET,
    path = "/api/clients",
}]
pub async fn list_clients(
    ctx: RequestContext<ApiContext>,
) -> Result<HttpResponseOk<ListClientsResponse>, HttpError> {
    let registry = &ctx.context().registry;
    let clients = registry.list_clients();

    Ok(HttpResponseOk(ListClientsResponse { clients }))
}
