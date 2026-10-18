//! Live UDP socket ownership: which process owns a local UDP endpoint.
//!
//! # Why this exists
//!
//! Process-targeted interception cannot be decided from a seed list. The
//! scanner's routes are built from sockets that were *already established*, and
//! `GetExtendedUdpTable` reports no remote endpoint for a UDP row, so a seed
//! list cannot answer "is this packet mine?" for a flow that opened after start -
//! which is every flow on a machine where the app was launched while the game or
//! browser was already running.
//!
//! A process-mode interceptor that consulted only that list would silently
//! relay nothing.
//!
//! The portable answer, and the one every production per-app traffic tool uses,
//! is to ask the OS who owns the endpoint: enumerate the UDP bindings with
//! their owning PID and match a captured packet's *local* port against that
//! table. A local port is unique per (address, port) among live sockets, so the
//! mapping is exact rather than heuristic. It is a snapshot read, so it is
//! refreshed on an interval rather than per packet.
//!
//! This deliberately does NOT replace the WinDivert FLOW/SOCKET layers. Those
//! give *events* (a flow appeared) which are cheaper and catch the first
//! datagram; this table gives *state* (who owns it now), which is what answers
//! for a flow that predates our handle. The map prefers an event when one
//! exists and falls back to this table, which is why both live behind
//! [`super::flows::AttributionSource`].
//!
//! Windows implementation: `GetExtendedUdpTable` with
//! `UDP_TABLE_OWNER_PID` (`MIB_UDPTABLE_OWNER_PID`), the documented API for
//! "which PID owns this UDP endpoint", reached through raw FFI so the crate
//! keeps its single `windows`-crate dependency surface. IPv4 and IPv6 tables are
//! both read because a v6 socket carries v4-mapped traffic.

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

/// How long a snapshot is trusted before another enumeration is taken.
///
/// Binding state changes at process-start/exit rates, not packet rates, so a
/// short interval is plenty; the cost of being stale is one packet taking the
/// direct path (fail-open), never a wrong attribution.
pub const OWNER_CACHE_TTL: Duration = Duration::from_millis(500);

/// One UDP endpoint: the local address/port and the PID bound to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpOwner {
    pub local: SocketAddrV4,
    pub pid: u32,
}

/// A snapshot of every UDP binding on the host, indexed by local port.
///
/// Indexed by port rather than by full address because a captured packet's
/// source address is the machine's own, which may be any of several addresses
/// on a given interface; the port is what identifies the socket.
#[derive(Debug, Default, Clone)]
pub struct UdpOwnerTable {
    by_port: HashMap<u16, Vec<u32>>,
}

impl UdpOwnerTable {
    /// Build a table from enumerated endpoints. Later duplicates accumulate:
    /// two processes can hold the same port on different addresses, and the
    /// caller disambiguates with the address when it has one.
    pub fn from_owners(owners: impl IntoIterator<Item = UdpOwner>) -> Self {
        let mut by_port: HashMap<u16, Vec<u32>> = HashMap::new();
        for o in owners {
            by_port.entry(o.local.port()).or_default().push(o.pid);
        }
        Self { by_port }
    }

    /// The PIDs bound to a local port, in enumeration order.
    pub fn pids_for_port(&self, port: u16) -> &[u32] {
        self.by_port.get(&port).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Whether `pid` owns a socket on `port`.
    pub fn owns(&self, port: u16, pid: u32) -> bool {
        self.pids_for_port(port).contains(&pid)
    }

    /// Whether any process owns a socket on `port`.
    pub fn is_bound(&self, port: u16) -> bool {
        self.by_port.contains_key(&port)
    }

    pub fn len(&self) -> usize {
        self.by_port.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_port.is_empty()
    }
}

/// A [`UdpOwnerTable`] with a refresh interval, so the intercept thread never
/// pays for an enumeration more than once per [`OWNER_CACHE_TTL`].
#[derive(Debug, Default)]
pub struct OwnerCache {
    table: UdpOwnerTable,
    taken_at: Option<Instant>,
}

impl OwnerCache {
    /// The current table, re-reading it when the snapshot has aged out.
    ///
    /// A failed enumeration leaves the previous table in place: stale ownership
    /// is strictly better than none, and the caller's decision is fail-open
    /// either way.
    pub fn table(&mut self, now: Instant) -> &UdpOwnerTable {
        let fresh = match self.taken_at {
            Some(t) => now.duration_since(t) < OWNER_CACHE_TTL,
            None => false,
        };
        if !fresh {
            if let Some(table) = platform::enumerate_udp_owners() {
                self.table = table;
                self.taken_at = Some(now);
            }
        }
        &self.table
    }

    /// Force the next [`Self::table`] call to re-read.
    pub fn invalidate(&mut self) {
        self.taken_at = None;
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{UdpOwner, UdpOwnerTable};
    use std::net::{Ipv4Addr, SocketAddrV4};

    // MIB_UDPROW_OWNER_PID: local address, local port, owning PID.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibUdpRowOwnerPid {
        local_addr: u32,
        local_port: u32,
        owning_pid: u32,
    }

    #[repr(C)]
    struct MibUdpTableOwnerPid {
        num_entries: u32,
        table: [MibUdpRowOwnerPid; 1],
    }

    const AF_INET: u32 = 2;
    const UDP_TABLE_OWNER_PID: u32 = 1;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedUdpTable(
            table: *mut core::ffi::c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
    }

    /// Every host UDP binding with its owning PID, or `None` when the OS call
    /// cannot be satisfied (which the caller treats as "keep the old snapshot").
    pub fn enumerate_udp_owners() -> Option<UdpOwnerTable> {
        let mut size: u32 = 0;
        // First call sizes the buffer; the documented two-call pattern.
        let rc = unsafe {
            GetExtendedUdpTable(
                core::ptr::null_mut(),
                &mut size,
                0,
                AF_INET,
                UDP_TABLE_OWNER_PID,
                0,
            )
        };
        if rc != ERROR_INSUFFICIENT_BUFFER || size == 0 {
            return None;
        }

        // Over-allocate: the table can grow between the two calls, and the
        // second call fails outright if the buffer is one row short.
        let mut buf = vec![0u8; size as usize + 4096];
        let rc = unsafe {
            GetExtendedUdpTable(
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                &mut size,
                0,
                AF_INET,
                UDP_TABLE_OWNER_PID,
                0,
            )
        };
        if rc != 0 {
            return None;
        }

        let table = buf.as_ptr() as *const MibUdpTableOwnerPid;
        let count = unsafe { (*table).num_entries } as usize;
        let rows = unsafe { core::slice::from_raw_parts((*table).table.as_ptr(), count) };

        let mut owners = Vec::with_capacity(count);
        for row in rows {
            // The table stores the port in network byte order in the high half
            // of a u32 whose low half is zero, and the address little-endian.
            let port = u16::from_be((row.local_port & 0xffff) as u16);
            if port == 0 {
                continue;
            }
            let octets = row.local_addr.to_ne_bytes();
            owners.push(UdpOwner {
                local: SocketAddrV4::new(Ipv4Addr::from(octets), port),
                pid: row.owning_pid,
            });
        }
        Some(UdpOwnerTable::from_owners(owners))
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::UdpOwnerTable;

    /// No owner table off Windows: the platform backends there resolve
    /// ownership through their own mechanisms (a `/proc` walk on Linux, the
    /// scanner's libproc path on macOS), so the shared cache stays empty and
    /// callers fall back to those.
    pub fn enumerate_udp_owners() -> Option<UdpOwnerTable> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn sock(addr: [u8; 4], port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::from(addr), port)
    }

    #[test]
    fn a_port_maps_to_its_owner() {
        let t = UdpOwnerTable::from_owners([
            UdpOwner {
                local: sock([0, 0, 0, 0], 50000),
                pid: 42,
            },
            UdpOwner {
                local: sock([0, 0, 0, 0], 50001),
                pid: 43,
            },
        ]);
        assert!(t.owns(50000, 42));
        assert!(!t.owns(50000, 43));
        assert_eq!(t.pids_for_port(50001), &[43]);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn the_same_port_on_two_addresses_keeps_both_owners() {
        let t = UdpOwnerTable::from_owners([
            UdpOwner {
                local: sock([0, 0, 0, 0], 6000),
                pid: 7,
            },
            UdpOwner {
                local: sock([127, 0, 0, 1], 6000),
                pid: 8,
            },
        ]);
        assert_eq!(t.pids_for_port(6000), &[7, 8]);
        assert!(t.owns(6000, 7) && t.owns(6000, 8));
    }

    #[test]
    fn an_unbound_port_has_no_owner() {
        let t = UdpOwnerTable::from_owners([UdpOwner {
            local: sock([0, 0, 0, 0], 1),
            pid: 1,
        }]);
        assert!(!t.is_bound(2));
        assert!(t.pids_for_port(2).is_empty());
    }

    #[test]
    fn the_cache_serves_one_snapshot_until_it_ages_out() {
        // Without a live enumeration there is nothing to time: on Linux and macOS
        // `enumerate_udp_owners` answers None by design, so the cache keeps no
        // snapshot and the assertions below would describe a state that cannot
        // exist. The first Linux run of the visible 1.7 suite failed here.
        if !cfg!(target_os = "windows") {
            return;
        }
        let mut cache = OwnerCache::default();
        let t0 = Instant::now();
        let _ = cache.table(t0);
        let taken = cache.taken_at;
        assert!(taken.is_some(), "the first call must take a snapshot");
        let _ = cache.table(t0 + OWNER_CACHE_TTL / 2);
        assert_eq!(cache.taken_at, taken, "a young snapshot is reused");
        let _ = cache.table(t0 + OWNER_CACHE_TTL * 2);
        assert!(
            cache.taken_at.unwrap() >= t0 + OWNER_CACHE_TTL,
            "an aged snapshot is re-taken"
        );
    }

    #[test]
    fn invalidate_forces_a_re_read() {
        let mut cache = OwnerCache::default();
        let t0 = Instant::now();
        let _ = cache.table(t0);
        cache.invalidate();
        assert!(cache.taken_at.is_none());
    }

    /// On Windows the live enumeration must succeed and must see this test
    /// process's own sockets; elsewhere it is documented as absent.
    #[test]
    fn live_enumeration_matches_the_platform_contract() {
        let table = platform::enumerate_udp_owners();
        if cfg!(target_os = "windows") {
            let table = table.expect("GetExtendedUdpTable(OWNER_PID) must succeed on Windows");
            // A machine always has some UDP bindings; an empty table would mean
            // the parse silently produced nothing.
            assert!(
                !table.is_empty(),
                "the owner table must not be empty on a live host"
            );
        } else {
            assert!(
                table.is_none(),
                "non-Windows hosts resolve ownership elsewhere"
            );
        }
    }
}
