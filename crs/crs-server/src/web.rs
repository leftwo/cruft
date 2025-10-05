// Copyright 2025 Oxide Computer Company

//! Web dashboard
//!
//! This module provides the HTML web dashboard endpoint for viewing
//! registered clients in a web browser.

// Suppress warnings for Dropshot's macro-generated phantom types
#![allow(dead_code)]

use crate::api::ApiContext;
use crs_common::ClientStatus;
use dropshot::{endpoint, Body, HttpError, RequestContext};
use http::{Response, StatusCode};

/// Serve the web dashboard
///
/// Generates an HTML page displaying all registered clients in a table
/// with their status, information, and last heartbeat time. The page
/// auto-refreshes every 10 seconds. Status is color-coded:
/// - Green: online (heartbeat within 60 seconds)
/// - Orange: stale (heartbeat 60-180 seconds ago)
/// - Red: offline (no heartbeat for 180+ seconds)
#[endpoint {
    method = GET,
    path = "/",
}]
pub async fn dashboard(
    ctx: RequestContext<ApiContext>,
) -> Result<Response<Body>, HttpError> {
    let api_context = ctx.context();
    let registry = &api_context.registry;
    let mut clients = registry.list_clients();

    // Sort clients by IP address
    clients.sort_by(|a, b| a.info.ip_address.cmp(&b.info.ip_address));

    // Get server information
    let server_hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let server_os = std::env::consts::OS;
    let server_version = env!("CARGO_PKG_VERSION");
    let server_bind_addr = ctx.server.local_addr;

    // Calculate server uptime
    let now = chrono::Utc::now();
    let uptime_duration = now - api_context.start_time;
    let uptime_str = if uptime_duration.num_days() > 0 {
        format!(
            "{}d {}h",
            uptime_duration.num_days(),
            uptime_duration.num_hours() % 24
        )
    } else if uptime_duration.num_hours() > 0 {
        format!(
            "{}h {}m",
            uptime_duration.num_hours(),
            uptime_duration.num_minutes() % 60
        )
    } else if uptime_duration.num_minutes() > 0 {
        format!("{}m", uptime_duration.num_minutes())
    } else {
        format!("{}s", uptime_duration.num_seconds())
    };

    let mut rows = String::new();
    for client in &clients {
        let status_color = match client.status {
            ClientStatus::Online => "green",
            ClientStatus::Stale => "orange",
            ClientStatus::Offline => "red",
        };

        let status_text = match client.status {
            ClientStatus::Online => "online",
            ClientStatus::Stale => "stale",
            ClientStatus::Offline => "offline",
        };

        // Calculate time connected
        // For offline clients, use last_heartbeat instead of now
        let end_time = if client.status == ClientStatus::Offline {
            client.last_heartbeat
        } else {
            chrono::Utc::now()
        };
        let connected_duration = end_time - client.registered_at;
        let connected_str = if connected_duration.num_days() > 0 {
            format!(
                "{}d {}h",
                connected_duration.num_days(),
                connected_duration.num_hours() % 24
            )
        } else if connected_duration.num_hours() > 0 {
            format!(
                "{}h {}m",
                connected_duration.num_hours(),
                connected_duration.num_minutes() % 60
            )
        } else if connected_duration.num_minutes() > 0 {
            format!("{}m", connected_duration.num_minutes())
        } else {
            format!("{}s", connected_duration.num_seconds())
        };

        rows.push_str(&format!(
            r#"
        <tr>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td style="color: {}; font-weight: bold;">{}</td>
            <td>{}</td>
            <td>{}</td>
        </tr>"#,
            client.info.hostname,
            client.info.ip_address,
            client.info.os,
            status_color,
            status_text,
            client.last_heartbeat.format("%Y-%m-%d %H:%M:%S UTC"),
            connected_str,
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Central Registry Service</title>
    <meta http-equiv="refresh" content="10">
    <style>
        body {{
            font-family: monospace;
            margin: 20px;
            background-color: #f5f5f5;
        }}
        h1 {{
            color: #333;
        }}
        h2 {{
            color: #555;
            margin-top: 30px;
        }}
        table {{
            border-collapse: collapse;
            width: 100%;
            background-color: white;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }}
        th, td {{
            border: 1px solid #ddd;
            padding: 8px;
            text-align: left;
        }}
        th {{
            background-color: #4CAF50;
            color: white;
        }}
        tr:nth-child(even) {{
            background-color: #f9f9f9;
        }}
        .info {{
            margin: 10px 0;
            color: #666;
        }}
        .server-info {{
            background-color: #e8f5e9;
        }}
    </style>
</head>
<body>
    <h1>Central Registry Service</h1>
    <div class="info">
        Page auto-refreshes every 10 seconds
    </div>

    <h2>Server Information</h2>
    <table>
        <tr>
            <th>Hostname</th>
            <th>IP Address</th>
            <th>OS</th>
            <th>Version</th>
            <th>Uptime</th>
        </tr>
        <tr class="server-info">
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
        </tr>
    </table>

    <h2>Registered Clients ({})</h2>
    <table>
        <tr>
            <th>Hostname</th>
            <th>IP Address</th>
            <th>OS</th>
            <th>Status</th>
            <th>Last Heartbeat</th>
            <th>Time Connected</th>
        </tr>
        {}
    </table>
</body>
</html>"#,
        server_hostname,
        server_bind_addr,
        server_os,
        server_version,
        uptime_str,
        clients.len(),
        rows
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(html.into())
        .map_err(|e| {
            HttpError::for_internal_error(format!(
                "failed to build response: {}",
                e
            ))
        })
}
