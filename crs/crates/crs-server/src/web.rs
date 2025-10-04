// Copyright 2025 Oxide Computer Company

//! Web dashboard

use crate::api::ApiContext;
use crs_common::ClientStatus;
use dropshot::{endpoint, HttpError, HttpResponseOk, RequestContext};

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
) -> Result<HttpResponseOk<String>, HttpError> {
    let registry = &ctx.context().registry;
    let clients = registry.list_clients();

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

        let tags = client
            .info
            .tags
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        rows.push_str(&format!(
            r#"
        <tr>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td>{}</td>
            <td style="color: {}; font-weight: bold;">{}</td>
            <td>{}</td>
            <td>{}</td>
        </tr>"#,
            client.client_id,
            client.info.hostname,
            client.info.os,
            client.info.ip_address,
            client.info.version,
            status_color,
            status_text,
            client.last_heartbeat.format("%Y-%m-%d %H:%M:%S UTC"),
            tags
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
        table {{
            border-collapse: collapse;
            width: 100%;
            background-color: white;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
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
    </style>
</head>
<body>
    <h1>Central Registry Service</h1>
    <div class="info">
        Total clients: {} | Page auto-refreshes every 10 seconds
    </div>
    <table>
        <tr>
            <th>Client ID</th>
            <th>Hostname</th>
            <th>OS</th>
            <th>IP Address</th>
            <th>Version</th>
            <th>Status</th>
            <th>Last Heartbeat</th>
            <th>Tags</th>
        </tr>
        {}
    </table>
</body>
</html>"#,
        clients.len(),
        rows
    );

    Ok(HttpResponseOk(html))
}
