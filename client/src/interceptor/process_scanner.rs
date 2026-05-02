//! Cross-platform game-process scanner.
//!
//! Scans running OS processes, matches them against known game executables, and
//! returns the active UDP sockets owned by those processes so the interceptor
//! can lock onto the right server IP:port automatically.
//!
//! ## Platform strategies
//!
//! | Platform | Process list   | Socket-to-PID mapping                        |
//! |----------|----------------|----------------------------------------------|
//! | Windows  | `tasklist /FO CSV` | `netstat -anou` (UDP + PID)             |
//! | Linux    | `/proc/<pid>/comm` | `/proc/<pid>/net/udp` or `ss -unp`      |
//! | macOS    | `ps -e -o pid,comm=` | `lsof -i UDP -n -P` or `netstat -anuvp` |
//!
//! The scanner intentionally avoids external crate deps (no `sysinfo`) to keep
//! the binary small and eliminate supply-chain risk.

use std::net::{Ipv4Addr, SocketAddrV4};

use super::traits::{ProcessInfo, Route, TransportProtocol};

// ─────────────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Scan the OS for any of the given process names and return matching
/// [`ProcessInfo`] entries with their live UDP routes.
///
/// `process_names` is a slice of image names to look for (case-insensitive),
/// e.g. `&["RustClient.exe", "cs2.exe"]`.
///
/// Returns an empty vec (never panics) if scanning fails.
pub fn scan_for_games(process_names: &[&str]) -> Vec<ProcessInfo> {
    let pid_list = list_pids_by_name(process_names);
    if pid_list.is_empty() {
        return vec![];
    }

    let udp_table = list_udp_sockets(); // (remote_ip, remote_port, local_port, pid)

    pid_list
        .into_iter()
        .map(|(pid, name)| {
            let routes = udp_table
                .iter()
                .filter(|(_, _, _, owner_pid)| *owner_pid == pid)
                .filter_map(|(rem_ip, rem_port, local_port, _)| {
                    // Only include routes to public remote IPs (not 0.0.0.0 listeners
                    // or loopback or RFC1918 private addresses).
                    if !is_public_ipv4(*rem_ip) {
                        return None;
                    }
                    Some(Route {
                        local: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, *local_port),
                        remote: SocketAddrV4::new(*rem_ip, *rem_port),
                        proto: TransportProtocol::Udp,
                    })
                })
                .collect();

            ProcessInfo { pid, name, routes }
        })
        .collect()
}

/// Find a single game process and return its PID + routes.
///
/// Helper that flattens `scan_for_games` to the first matching process.
pub fn find_game_process(process_names: &[&str]) -> Option<ProcessInfo> {
    let results = scan_for_games(process_names);
    results.into_iter().next()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` when `ip` is a routable public IPv4 address.
///
/// Rejects: unspecified (0.0.0.0), loopback (127.x), link-local (169.254.x),
/// and the three RFC 1918 private ranges.
pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_link_local() {
        return false;
    }
    let [a, b, ..] = ip.octets();
    // 10.0.0.0/8
    if a == 10 {
        return false;
    }
    // 172.16.0.0/12
    if a == 172 && (16..=31).contains(&b) {
        return false;
    }
    // 192.168.0.0/16
    if a == 192 && b == 168 {
        return false;
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  Platform-specific: PID listing
// ─────────────────────────────────────────────────────────────────────────────

/// Return `(pid, process_name)` pairs for processes whose image name matches
/// any entry in `names` (case-insensitive comparison).
fn list_pids_by_name(names: &[&str]) -> Vec<(u32, String)> {
    #[cfg(target_os = "windows")]
    return list_pids_windows(names);

    #[cfg(target_os = "linux")]
    return list_pids_linux(names);

    #[cfg(target_os = "macos")]
    return list_pids_macos(names);

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = names;
        vec![]
    }
}

/// Windows: parse `tasklist /FO CSV /NH`.
///
/// CSV columns: `"ImageName","PID","SessionName","Session#","Mem Usage"`
#[cfg(target_os = "windows")]
fn list_pids_windows(names: &[&str]) -> Vec<(u32, String)> {
    let output = match std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            // Each line: "ImageName.exe","1234","..."
            let fields: Vec<&str> = line.trim().split(',').collect();
            if fields.len() < 2 {
                return None;
            }
            let img = fields[0].trim_matches('"');
            let pid_str = fields[1].trim_matches('"');
            let pid: u32 = pid_str.parse().ok()?;
            if names.iter().any(|n| n.eq_ignore_ascii_case(img)) {
                Some((pid, img.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Linux: walk `/proc/<pid>/comm` for the process name, then match.
#[cfg(target_os = "linux")]
fn list_pids_linux(names: &[&str]) -> Vec<(u32, String)> {
    let mut result = Vec::new();
    let proc = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return result,
    };
    for entry in proc.flatten() {
        let file_name = entry.file_name();
        let pid_str = file_name.to_string_lossy();
        let pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => continue, // not a numeric PID directory
        };
        let comm_path = format!("/proc/{}/comm", pid);
        if let Ok(comm) = std::fs::read_to_string(&comm_path) {
            let comm = comm.trim();
            if names.iter().any(|n| n.eq_ignore_ascii_case(comm)) {
                result.push((pid, comm.to_string()));
            }
        }
    }
    result
}

/// macOS: parse `ps -e -o pid,comm=` output.
#[cfg(target_os = "macos")]
fn list_pids_macos(names: &[&str]) -> Vec<(u32, String)> {
    let output = match std::process::Command::new("ps")
        .args(["-e", "-o", "pid=,comm="])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let mut parts = line.splitn(2, ' ');
            let pid: u32 = parts.next()?.trim().parse().ok()?;
            let comm = parts.next()?.trim();
            // comm may be a full path — take just the basename
            let basename = comm.rsplit('/').next().unwrap_or(comm);
            if names.iter().any(|n| n.eq_ignore_ascii_case(basename)) {
                Some((pid, basename.to_string()))
            } else {
                None
            }
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Platform-specific: UDP socket table
// ─────────────────────────────────────────────────────────────────────────────

/// Return `(remote_ip, remote_port, local_port, pid)` for all active UDP
/// sockets that have a non-zero remote address (i.e., connected sockets).
fn list_udp_sockets() -> Vec<(Ipv4Addr, u16, u16, u32)> {
    #[cfg(target_os = "windows")]
    return list_udp_windows();

    #[cfg(target_os = "linux")]
    return list_udp_linux();

    #[cfg(target_os = "macos")]
    return list_udp_macos();

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    vec![]
}

/// Windows: parse `netstat -anou -p UDP`.
///
/// Relevant output lines:
/// ```text
///   UDP    192.168.1.5:54321      1.2.3.4:28015          *          1234
/// ```
/// Columns (after stripping leading whitespace):
/// `Proto LocalAddress RemoteAddress State(optional) PID`
///
/// For UDP, Windows `netstat -ano` output looks like:
/// ```text
///   UDP    0.0.0.0:28015          *:*                    1234
/// ```
/// The remote address is `*:*` for listening sockets; for "connected" UDP it
/// shows the actual remote IP.  We want connected entries only.
#[cfg(target_os = "windows")]
fn list_udp_windows() -> Vec<(Ipv4Addr, u16, u16, u32)> {
    // Use `netstat -ano -p UDP` — includes PID in the last column.
    let output = match std::process::Command::new("netstat")
        .args(["-ano", "-p", "UDP"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let mut result = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Expected: ["UDP", "local:port", "remote:port", "PID"]
        // State field may or may not be present.
        if parts.len() < 4 {
            continue;
        }
        if !parts[0].eq_ignore_ascii_case("UDP") {
            continue;
        }
        let local_str = parts[1];
        let remote_str = parts[2];
        // PID is the last field
        let pid: u32 = match parts.last().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };

        // Skip listening sockets — remote is "*:*" or "0.0.0.0:*"
        if remote_str.contains('*') || remote_str.starts_with("0.0.0.0:0") {
            continue;
        }

        let (local_port, remote_ip, remote_port) =
            match parse_local_remote_windows(local_str, remote_str) {
                Some(t) => t,
                None => continue,
            };

        result.push((remote_ip, remote_port, local_port, pid));
    }
    result
}

/// Parse `"192.168.1.5:54321"` → `(54321)` and `"1.2.3.4:28015"` → `(ip, 28015)`.
/// IPv4 only.
#[cfg(target_os = "windows")]
fn parse_local_remote_windows(local: &str, remote: &str) -> Option<(u16, Ipv4Addr, u16)> {
    let local_port = local.rsplit(':').next()?.parse::<u16>().ok()?;

    let colon = remote.rfind(':')?;
    let rip_str = &remote[..colon];
    let rport_str = &remote[colon + 1..];
    let remote_port: u16 = rport_str.parse().ok()?;
    let remote_ip: Ipv4Addr = rip_str.parse().ok()?;

    Some((local_port, remote_ip, remote_port))
}

/// Linux: parse `/proc/<pid>/net/udp` for all PIDs we found.
///
/// The file format (space-separated):
/// ```text
/// sl  local_address rem_address   st tx_queue rx_queue tr tm inode
///  0: 00000000:6D87 1C02040A:E86F 01 ...
/// ```
/// Addresses are in little-endian hex: `AABBCCDD:PPPP`.
/// We want rows with `st != 07` (07 = CLOSE / listening), i.e., st=01 means
/// `ESTABLISHED` (connected UDP socket).
///
/// Since `/proc/net/udp` is global (all PIDs), we match by inode to the
/// per-PID `/proc/<pid>/fd/` symlinks.  For simplicity we use `ss -unp` which
/// directly shows PID in its output.
#[cfg(target_os = "linux")]
fn list_udp_linux() -> Vec<(Ipv4Addr, u16, u16, u32)> {
    // Try `ss -unp` first (fast, shows PID directly).
    if let Some(sockets) = list_udp_linux_ss() {
        return sockets;
    }
    // Fallback: parse /proc/net/udp + /proc/<pid>/fd inode matching.
    list_udp_linux_proc()
}

/// `ss -unp` output.  Example for connected UDP:
/// ```text
/// ESTAB  0  0  192.168.1.5:54321  1.2.3.4:28015  users:(("RustClient",pid=1234,fd=5))
/// ```
#[cfg(target_os = "linux")]
fn list_udp_linux_ss() -> Option<Vec<(Ipv4Addr, u16, u16, u32)>> {
    let output = std::process::Command::new("ss")
        .args(["-unp"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut result = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Skip header
        if line.starts_with("Netid") || line.starts_with("State") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Columns: State RecvQ SendQ LocalAddr:Port PeerAddr:Port users:(...)
        if parts.len() < 5 {
            continue;
        }
        // Only ESTAB (connected) UDP sockets
        if !parts[0].eq_ignore_ascii_case("ESTAB") {
            continue;
        }

        let local_str = parts[3];
        let remote_str = parts[4];
        let users_str = parts.get(5).copied().unwrap_or("");

        // Extract PID from users field: `users:(("proc",pid=N,fd=M))`
        let pid = extract_pid_from_ss_users(users_str)?;

        let (local_port, remote_ip, remote_port) = parse_addr_port_linux(local_str, remote_str)?;

        result.push((remote_ip, remote_port, local_port, pid));
    }
    Some(result)
}

#[cfg(target_os = "linux")]
fn extract_pid_from_ss_users(s: &str) -> Option<u32> {
    // s looks like: users:(("prog",pid=1234,fd=5))
    let start = s.find("pid=")?;
    let rest = &s[start + 4..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(target_os = "linux")]
fn parse_addr_port_linux(local: &str, remote: &str) -> Option<(u16, Ipv4Addr, u16)> {
    // Format: "ip:port" or "[::1]:port"  — we only care about IPv4.
    let local_port = local.rsplit(':').next()?.parse::<u16>().ok()?;

    let colon = remote.rfind(':')?;
    let rip_str = &remote[..colon];
    let rport_str = &remote[colon + 1..];
    let remote_port: u16 = rport_str.parse().ok()?;
    let remote_ip: Ipv4Addr = rip_str
        .trim_matches(|c| c == '[' || c == ']')
        .parse()
        .ok()?;

    Some((local_port, remote_ip, remote_port))
}

/// Fallback: parse `/proc/net/udp` with inode→PID matching.
#[cfg(target_os = "linux")]
fn list_udp_linux_proc() -> Vec<(Ipv4Addr, u16, u16, u32)> {
    let udp_content = match std::fs::read_to_string("/proc/net/udp") {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // Build inode → PID map from /proc/<pid>/fd symlinks
    let inode_pid = build_inode_pid_map();

    let mut result = Vec::new();
    for line in udp_content.lines().skip(1) {
        // Fields: sl local_address rem_address st tx:rx tr tm inode ...
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        let status = fields[3]; // "01" = ESTABLISHED, "07" = CLOSE (listening)
        if status == "07" {
            continue;
        }

        let local_hex = fields[1];
        let remote_hex = fields[2];
        let inode_str = fields[9];
        let inode: u64 = match inode_str.parse() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let pid = match inode_pid.get(&inode) {
            Some(&p) => p,
            None => continue,
        };

        let (local_ip, local_port) = match parse_hex_addr(local_hex) {
            Some(v) => v,
            None => continue,
        };
        let (remote_ip, remote_port) = match parse_hex_addr(remote_hex) {
            Some(v) => v,
            None => continue,
        };

        if !is_public_ipv4(remote_ip) {
            continue;
        }
        let _ = local_ip;
        result.push((remote_ip, remote_port, local_port, pid));
    }
    result
}

/// Linux: build `inode → pid` map by iterating `/proc/<pid>/fd` symlinks.
#[cfg(target_os = "linux")]
fn build_inode_pid_map() -> std::collections::HashMap<u64, u32> {
    let mut map = std::collections::HashMap::new();
    let proc = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return map,
    };
    for entry in proc.flatten() {
        let n = entry.file_name();
        let pid: u32 = match n.to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let fd_dir = format!("/proc/{}/fd", pid);
        let fds = match std::fs::read_dir(&fd_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for fd in fds.flatten() {
            if let Ok(link) = std::fs::read_link(fd.path()) {
                let s = link.to_string_lossy();
                // socket:[12345]
                if s.starts_with("socket:[") {
                    if let Some(inode_str) =
                        s.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']'))
                    {
                        if let Ok(inode) = inode_str.parse::<u64>() {
                            map.insert(inode, pid);
                        }
                    }
                }
            }
        }
    }
    map
}

/// Parse a Linux `/proc/net/udp` hex address `"0F02010A:B8D2"`.
///
/// The address is stored little-endian: `0F02010A` = `10.1.2.15`.
/// We reverse the byte order to get the correct IP.
#[cfg(target_os = "linux")]
fn parse_hex_addr(s: &str) -> Option<(Ipv4Addr, u16)> {
    let (ip_hex, port_hex) = s.split_once(':')?;
    let ip_raw = u32::from_str_radix(ip_hex, 16).ok()?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    // Little-endian: reverse bytes
    let ip = Ipv4Addr::from(ip_raw.to_be());
    Some((ip, port))
}

// Allow dead_code suppression on Linux-only fn when building for non-Linux.
#[allow(dead_code)]
#[cfg(not(target_os = "linux"))]
fn parse_hex_addr(_s: &str) -> Option<(Ipv4Addr, u16)> {
    None
}

/// macOS: parse `netstat -anup` or fall back to `lsof -i UDP -n -P`.
///
/// `netstat -anup` example line:
/// ```
/// udp4       0      0  192.168.1.5.54321      1.2.3.4.28015          *.*
/// ```
/// Port separator is `.` (dot), not `:`.
#[cfg(target_os = "macos")]
fn list_udp_macos() -> Vec<(Ipv4Addr, u16, u16, u32)> {
    // Try lsof first — gives PID directly and is more reliable.
    if let Some(r) = list_udp_macos_lsof() {
        return r;
    }
    vec![]
}

#[cfg(target_os = "macos")]
fn list_udp_macos_lsof() -> Option<Vec<(Ipv4Addr, u16, u16, u32)>> {
    // lsof -i UDP -n -P  (no DNS lookup, numeric ports)
    // Example output line:
    //   RustClien 1234 user  UDP  *:*
    //   RustClien 1234 user  UDP  192.168.1.5:54321->1.2.3.4:28015 (ESTABLISHED)
    let output = std::process::Command::new("lsof")
        .args(["-i", "UDP", "-n", "-P"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut result = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Columns: COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        let pid: u32 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let name_field = parts[8]; // e.g. "192.168.1.5:54321->1.2.3.4:28015"
        if !name_field.contains("->") {
            continue; // Listening socket — skip
        }
        let mut halves = name_field.splitn(2, "->");
        let local_str = halves.next()?;
        let remote_str = halves.next()?;

        let local_port = local_str.rsplit(':').next()?.parse::<u16>().ok()?;
        let colon = remote_str.rfind(':')?;
        let remote_ip: Ipv4Addr = remote_str[..colon].parse().ok()?;
        let remote_port: u16 = remote_str[colon + 1..].parse().ok()?;

        result.push((remote_ip, remote_port, local_port, pid));
    }
    Some(result)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_public_ipv4() {
        assert!(is_public_ipv4("8.8.8.8".parse().unwrap()));
        assert!(is_public_ipv4("1.1.1.1".parse().unwrap()));
        assert!(!is_public_ipv4("10.0.0.1".parse().unwrap()));
        assert!(!is_public_ipv4("172.16.0.1".parse().unwrap()));
        assert!(!is_public_ipv4("192.168.1.1".parse().unwrap()));
        assert!(!is_public_ipv4("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ipv4("0.0.0.0".parse().unwrap()));
        assert!(!is_public_ipv4("169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn test_scan_doesnt_panic() {
        // On CI there are no game processes — that's fine.
        let _ = scan_for_games(&["RustClient.exe", "cs2.exe"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_hex_addr() {
        // 0x7F000001 in little-endian = 127.0.0.1, port 0x0035 = 53
        let result = super::parse_hex_addr("0100007F:0035");
        assert!(result.is_some());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, std::net::Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(port, 53);
    }
}
