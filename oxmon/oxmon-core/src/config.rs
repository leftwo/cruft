use anyhow::{Context, Result};
use oxmon_common::HostConfig;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::path::Path;

/// Load hosts from a CSV file
/// Format: hostname,ip_address
/// Lines starting with # are comments
pub fn load_hosts_from_file(path: &Path) -> Result<Vec<HostConfig>> {
    let file = File::open(path)
        .context(format!("Failed to open file: {}", path.display()))?;

    let reader = BufReader::new(file);
    let mut hosts = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "Invalid format on line {}: expected 'hostname,ip_address'",
                line_num + 1
            );
        }

        let hostname = parts[0].trim().to_string();
        let ip_str = parts[1].trim();

        let ip_address: IpAddr = ip_str.parse().context(format!(
            "Invalid IP address '{}' on line {}",
            ip_str,
            line_num + 1
        ))?;

        hosts.push(HostConfig {
            hostname,
            ip_address,
        });
    }

    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{IpAddr, Ipv4Addr};
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_hosts_valid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();

        let hosts = load_hosts_from_file(file.path()).unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].hostname, "host1");
        assert_eq!(
            hosts[0].ip_address,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))
        );
        assert_eq!(hosts[1].hostname, "host2");
        assert_eq!(hosts[1].ip_address, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn test_load_hosts_with_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# This is a comment").unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file, "# Another comment").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();

        let hosts = load_hosts_from_file(file.path()).unwrap();
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn test_load_hosts_with_empty_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,192.168.1.1").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();

        let hosts = load_hosts_from_file(file.path()).unwrap();
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn test_load_hosts_with_whitespace() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "  host1  ,  192.168.1.1  ").unwrap();
        writeln!(file, "host2,10.0.0.1").unwrap();

        let hosts = load_hosts_from_file(file.path()).unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].hostname, "host1");
    }

    #[test]
    fn test_load_hosts_invalid_format() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "invalid_line_no_comma").unwrap();

        let result = load_hosts_from_file(file.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid format"));
    }

    #[test]
    fn test_load_hosts_invalid_ip() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,not.an.ip.address").unwrap();

        let result = load_hosts_from_file(file.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid IP address")
        );
    }

    #[test]
    fn test_load_hosts_file_not_found() {
        let result = load_hosts_from_file(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to open file")
        );
    }

    #[test]
    fn test_load_hosts_ipv6() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "host1,::1").unwrap();
        writeln!(file, "host2,2001:db8::1").unwrap();

        let hosts = load_hosts_from_file(file.path()).unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].hostname, "host1");
        assert!(hosts[0].ip_address.is_ipv6());
    }
}
