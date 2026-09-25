//! macOS pf anchor rule construction.
//!
//! This module is free of OS calls so the ordering and matching of the rules can
//! be tested on any host; `macos.rs` is the only production caller.
//!
//! The interceptor's redirect matches on destination IP:port only, so the
//! shadow-direct probe's own copy would be captured by it. A pf `no rdr` rule
//! keyed on the probe socket's source port, emitted *before* the redirect,
//! exempts exactly that socket.

use std::net::SocketAddrV4;

/// Build the pf anchor script for one locked game server.
///
/// When `probe_port` is `Some`, a `no rdr` rule matching that source port to the
/// exact server is emitted first so the shadow-direct probe escapes the
/// redirect; the destination-only `rdr pass` rule then follows unchanged.
pub fn anchor_script(server: SocketAddrV4, local_port: u16, probe_port: Option<u16>) -> String {
    let ip = server.ip();
    let port = server.port();
    let mut script = String::new();
    if let Some(probe_port) = probe_port {
        script.push_str(&format!(
            "no rdr proto udp from any port {probe_port} to {ip} port {port}\n"
        ));
    }
    script.push_str(&format!(
        "rdr pass proto udp from any to {ip} port {port} -> 127.0.0.1 port {local_port}\n"
    ));
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn server() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 34568)
    }

    #[test]
    fn redirect_rule_is_unchanged_without_a_probe() {
        let script = anchor_script(server(), 40000, None);
        assert_eq!(
            script,
            "rdr pass proto udp from any to 203.0.113.9 port 34568 -> 127.0.0.1 port 40000\n"
        );
    }

    #[test]
    fn probe_exemption_precedes_the_redirect() {
        let script = anchor_script(server(), 40000, Some(51234));
        let no_rdr = script
            .find("no rdr proto udp from any port 51234")
            .expect("the probe exemption must be present");
        let redirect = script
            .find("rdr pass proto udp from any to 203.0.113.9 port 34568")
            .expect("the redirect must still be present");
        assert!(
            no_rdr < redirect,
            "the no-rdr exemption must be evaluated before the redirect"
        );
    }

    #[test]
    fn probe_exemption_is_source_port_scoped_to_the_exact_server() {
        let script = anchor_script(server(), 40000, Some(51234));
        assert!(script.contains("no rdr proto udp from any port 51234 to 203.0.113.9 port 34568"));
        assert!(
            !script.contains("no rdr proto udp from any to 203.0.113.9"),
            "the exemption must be scoped to the probe's source port, or it would exempt all game traffic"
        );
    }
}
