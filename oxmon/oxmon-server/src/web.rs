use oxmon_common::{HostStatus, Status};

pub fn render_dashboard(hosts: &[HostStatus]) -> String {
    let mut sorted_hosts = hosts.to_vec();
    sorted_hosts.sort_by_key(|host| host.ip_address);

    let rows = sorted_hosts
        .iter()
        .map(|host| {
            let status_class = match host.status {
                Status::Online => "online",
                Status::Offline => "offline",
            };

            format!(
                r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td class="{}"><span class="status-circle"></span></td>
                </tr>
                "#,
                host.hostname, host.ip_address, status_class,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>OxMon - Network Monitoring</title>
    <meta charset="utf-8">
    <meta http-equiv="refresh" content="10">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI",
                Roboto, sans-serif;
            margin: 0;
            padding: 20px;
            background: #f5f5f5;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            padding: 30px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
        h1 {{
            margin: 0 0 10px 0;
            color: #333;
        }}
        .subtitle {{
            color: #666;
            margin-bottom: 20px;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
        }}
        th {{
            background: #333;
            color: white;
            padding: 12px;
            text-align: left;
            font-weight: 600;
        }}
        td {{
            padding: 12px;
            border-bottom: 1px solid #e0e0e0;
        }}
        tr:hover {{
            background: #f9f9f9;
        }}
        .online .status-circle {{
            background: #22c55e;
        }}
        .offline .status-circle {{
            background: #ef4444;
        }}
        .status-circle {{
            display: inline-block;
            width: 16px;
            height: 16px;
            border-radius: 50%;
        }}
        .footer {{
            margin-top: 20px;
            padding-top: 20px;
            border-top: 1px solid #e0e0e0;
            color: #666;
            font-size: 14px;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>OxMon Network Monitoring</h1>
        <div class="subtitle">
            Monitoring {} hosts | Auto-refresh every 10 seconds
        </div>
        <table>
            <thead>
                <tr>
                    <th>Hostname</th>
                    <th>IP Address</th>
                    <th>Status</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
        <div class="footer">
            Last updated: {} UTC | Pings sent every 10 seconds
                (3 pings, 5s timeout)
        </div>
    </div>
</body>
</html>"#,
        hosts.len(),
        rows,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
    )
}
