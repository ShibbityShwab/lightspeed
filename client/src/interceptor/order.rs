//! Pure, I/O-free server-tracking state machine for WinDivert interception.
//!
//! The Windows interceptor has to answer one question for every captured
//! outbound packet: *should this packet be tunnelled to the proxy, or
//! re-injected onto the wire untouched?* Historically that decision was
//! duplicated as ad-hoc mutable state inside two blocking receive loops
//! ([`crate::interceptor::windows`] and
//! [`crate::capture::windivert_redirect`]), and the duplication carried a
//! live bug: a server pre-seeded from `initial_routes.first()` was never
//! considered stale because `last_server_pkt` stayed `None` until a packet
//! matched its exact address, so the interceptor locked onto a dead lobby IP
//! forever and passed live-match traffic through untunnelled.
//!
//! This module is the single source of truth for that decision. It performs no
//! I/O and takes `now` as an explicit parameter, so the full state machine is
//! deterministically testable on every platform (the WinDivert loops
//! themselves are Windows-only and cannot run under CI on Linux).

use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// What the caller should do with the packet that was just observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Forward this packet to the locked `server` through the tunnel.
    Tunnel(SocketAddrV4),
    /// Re-inject the packet unchanged; no interesting state transition.
    PassThrough,
    /// Entering a fresh detection window: re-inject and keep accumulating.
    StartDetection,
    /// Sample this packet as a like-for-like direct application RTT probe.
    ///
    /// The caller re-injects the game's OWN packet UNCHANGED onto the direct
    /// path: no synthetic packet is generated and no payload is rewritten,
    /// which is the anti-cheat safety property. The sampler times the reply
    /// on a sniff handle, so from the game's point of view nothing changed.
    /// Distinct from [`PassThrough`](Decision::PassThrough) so the detection
    /// state machine's semantics stay untouched.
    ShadowDirect,
    /// The previous lock expired (server went stale); the caller must tear down
    /// per-session cached state (e.g. the inject interface cache) before this
    /// packet, which is re-injected and begins a new detection window.
    ResetToDetection,
}

/// Tuning for the detection and staleness timers.
#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// Packets to one destination required before locking onto it.
    pub detect_pkts: u8,
    /// Detection window: candidate counts older than this are discarded.
    pub detect_window: Duration,
    /// Silence (from either the last packet or the lock time) after which a
    /// locked server is considered gone.
    pub stale_timeout: Duration,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        // Matches the historical WinDivert constants: 3 packets within 1500 ms
        // lock a server; 5 s of silence releases it.
        Self {
            detect_pkts: 3,
            detect_window: Duration::from_millis(1_500),
            stale_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    addr: SocketAddrV4,
    count: u8,
    first_seen: Instant,
}

/// Server-tracking state machine.
///
/// Feed it every captured outbound packet in order via [`observe`] and act on
/// the returned [`Decision`].
///
/// [`observe`]: ServerTracker::observe
#[derive(Debug)]
pub struct ServerTracker {
    cfg: TrackerConfig,
    locked: Option<SocketAddrV4>,
    locked_at: Option<Instant>,
    last_server_pkt: Option<Instant>,
    candidates: Vec<Candidate>,
    seeds: Vec<SocketAddrV4>,
    reset_pending: bool,
}

/// Compute the seed routes the tracker should pre-lock onto.
///
/// Games with `dynamic_server() == true` (e.g. Fortnite) rotate through
/// ephemeral AWS addresses, so a discovered route is worthless the moment the
/// match starts: such games must begin in auto-detect and ignore the seeds.
/// Static games keep the legacy behaviour of starting from the discovered
/// routes. The *whole* route list is returned rather than collapsing to
/// `.first()` so callers never encode that assumption themselves.
pub fn effective_seeds(dynamic: bool, initial_routes: &[SocketAddrV4]) -> Vec<SocketAddrV4> {
    if dynamic {
        Vec::new()
    } else {
        initial_routes.to_vec()
    }
}

impl ServerTracker {
    /// Create an unlocked tracker.
    pub fn new(cfg: TrackerConfig) -> Self {
        Self {
            cfg,
            locked: None,
            locked_at: None,
            last_server_pkt: None,
            candidates: Vec::new(),
            seeds: Vec::new(),
            reset_pending: false,
        }
    }

    /// Pre-seed a lock. `routes[0]`, if present, becomes the locked server and
    /// the stale timer is anchored at `now` so a pre-seed that never receives
    /// a packet still expires.
    pub fn seed(&mut self, routes: &[SocketAddrV4], now: Instant) {
        self.seeds = routes.to_vec();
        if let Some(first) = routes.first().copied() {
            self.locked = Some(first);
            self.locked_at = Some(now);
            self.last_server_pkt = None;
        }
    }

    /// The currently locked server, if any.
    pub fn locked_server(&self) -> Option<SocketAddrV4> {
        self.locked
    }

    /// Whether a server is currently locked.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// The routes supplied to [`seed`], if any.
    ///
    /// [`seed`]: ServerTracker::seed
    pub fn seeded_routes(&self) -> &[SocketAddrV4] {
        &self.seeds
    }

    /// Number of live detection candidates.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Observe one outbound packet and return the routing decision.
    pub fn observe(
        &mut self,
        observed_dst: SocketAddrV4,
        now: Instant,
        payload_len: usize,
    ) -> Decision {
        if payload_len == 0 {
            return Decision::PassThrough;
        }

        if let Some(server) = self.locked {
            if observed_dst == server {
                self.last_server_pkt = Some(now);
                return Decision::Tunnel(server);
            }
            let anchor = self.last_server_pkt.or(self.locked_at);
            let stale =
                anchor.is_none_or(|t| now.saturating_duration_since(t) > self.cfg.stale_timeout);
            if stale {
                self.locked = None;
                self.locked_at = None;
                self.last_server_pkt = None;
                self.candidates.clear();
                self.reset_pending = true;
            } else {
                return Decision::PassThrough;
            }
        }

        self.expire_candidates(now);
        let started = self.candidates.is_empty();
        let committed = self.note_candidate(observed_dst, now);
        if committed {
            self.locked = Some(observed_dst);
            self.locked_at = Some(now);
            self.last_server_pkt = Some(now);
            self.candidates.clear();
            self.reset_pending = false;
            return Decision::Tunnel(observed_dst);
        }
        if self.reset_pending {
            self.reset_pending = false;
            return Decision::ResetToDetection;
        }
        if started {
            return Decision::StartDetection;
        }
        Decision::PassThrough
    }

    fn expire_candidates(&mut self, now: Instant) {
        let window = self.cfg.detect_window;
        self.candidates
            .retain(|c| now.saturating_duration_since(c.first_seen) < window);
    }

    fn note_candidate(&mut self, dst: SocketAddrV4, now: Instant) -> bool {
        if let Some(c) = self.candidates.iter_mut().find(|c| c.addr == dst) {
            c.count = c.count.saturating_add(1);
            c.count >= self.cfg.detect_pkts
        } else {
            self.candidates.push(Candidate {
                addr: dst,
                count: 1,
                first_seen: now,
            });
            self.cfg.detect_pkts <= 1
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn addr(a: u8, port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, a), port)
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn tracker() -> ServerTracker {
        ServerTracker::new(TrackerConfig::default())
    }

    // ── (a) single-server lock after N packets within the window ───────────
    #[test]
    fn three_packets_within_window_lock_and_tunnel() {
        let t0 = Instant::now();
        let a = addr(1, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        assert_eq!(t.observe(a, at(t0, 10), 100), Decision::PassThrough);
        assert_eq!(t.observe(a, at(t0, 20), 100), Decision::Tunnel(a));
        assert_eq!(t.locked_server(), Some(a));
        assert!(t.is_locked());

        // Subsequent packets to the locked server keep tunnelling.
        assert_eq!(t.observe(a, at(t0, 30), 100), Decision::Tunnel(a));
    }

    #[test]
    fn packets_spread_past_window_never_lock() {
        let t0 = Instant::now();
        let a = addr(2, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        // 2 s apart > 1500 ms window: the count keeps resetting.
        assert_eq!(t.observe(a, at(t0, 2000), 100), Decision::StartDetection);
        assert_eq!(t.observe(a, at(t0, 4000), 100), Decision::StartDetection);
        assert!(!t.is_locked(), "spread-out packets must not lock a server");
    }

    // ── (b) stale after 5 s of silence triggers re-detect ──────────────────
    #[test]
    fn stale_after_silence_triggers_reset_and_redetect() {
        let t0 = Instant::now();
        let a = addr(3, 7777);
        let b = addr(4, 7777);
        let mut t = tracker();

        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // A still sending, then 6 s of silence, then traffic to B.
        assert_eq!(t.observe(a, at(t0, 100), 100), Decision::Tunnel(a));
        assert_eq!(t.observe(b, at(t0, 6100), 100), Decision::ResetToDetection);
        assert!(!t.is_locked(), "stale lock must be released");

        assert_eq!(t.observe(b, at(t0, 6110), 100), Decision::PassThrough);
        assert_eq!(t.observe(b, at(t0, 6120), 100), Decision::Tunnel(b));
        assert_eq!(t.locked_server(), Some(b));
    }

    // ── (c) PRE-SEEDED server that never receives a packet (core bug) ──────
    #[test]
    fn preseeded_server_that_never_packets_goes_stale_and_redetects() {
        let t0 = Instant::now();
        let dead = addr(5, 7777);
        let live = addr(6, 7777);
        let mut t = tracker();
        t.seed(&[dead], t0);

        assert_eq!(t.locked_server(), Some(dead));
        assert!(t.is_locked());

        // No packet ever reaches `dead`. Just 6 s later, traffic to the live
        // server must force a reset rather than passing through forever.
        assert_eq!(
            t.observe(live, at(t0, 6000), 100),
            Decision::ResetToDetection
        );
        assert!(!t.is_locked());

        assert_eq!(t.observe(live, at(t0, 6010), 100), Decision::PassThrough);
        assert_eq!(t.observe(live, at(t0, 6020), 100), Decision::Tunnel(live));
        assert_eq!(t.locked_server(), Some(live));
    }

    #[test]
    fn preseeded_server_tunnels_on_first_matching_packet() {
        let t0 = Instant::now();
        let server = addr(7, 7777);
        let mut t = tracker();
        t.seed(&[server], t0);

        // The stale timer must NOT release a server that is actively talking.
        assert_eq!(t.observe(server, at(t0, 10), 100), Decision::Tunnel(server));
    }

    // ── (d) destination change while the old server is still sending ───────
    #[test]
    fn destination_change_while_old_server_active_passes_through() {
        let t0 = Instant::now();
        let a = addr(8, 7777);
        let b = addr(9, 7777);
        let mut t = tracker();
        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // B appears while A is fresh: pass it through, stay locked on A.
        assert_eq!(t.observe(b, at(t0, 100), 100), Decision::PassThrough);
        assert_eq!(t.locked_server(), Some(a));
        assert_eq!(t.observe(a, at(t0, 200), 100), Decision::Tunnel(a));
        assert_eq!(t.locked_server(), Some(a));
    }

    // ── (e) multi-route candidate set ──────────────────────────────────────
    #[test]
    fn multi_route_candidates_accumulate_independently() {
        let t0 = Instant::now();
        let a = addr(10, 7777);
        let b = addr(11, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        assert_eq!(t.observe(b, at(t0, 1), 100), Decision::PassThrough);
        assert_eq!(t.observe(a, at(t0, 2), 100), Decision::PassThrough);
        assert_eq!(t.observe(b, at(t0, 3), 100), Decision::PassThrough);
        // B reaches the threshold first and wins.
        assert_eq!(t.observe(b, at(t0, 4), 100), Decision::Tunnel(b));
        assert_eq!(t.locked_server(), Some(b));

        // A (only 2 packets) is still fresh but is not the locked server.
        assert_eq!(t.observe(a, at(t0, 5), 100), Decision::PassThrough);
        assert_eq!(t.locked_server(), Some(b));
    }

    #[test]
    fn effective_seeds_does_not_collapse_to_first_route() {
        let routes = vec![addr(20, 7777), addr(21, 7777), addr(22, 7777)];
        let seeds = effective_seeds(false, &routes);
        assert_eq!(seeds, routes, "static games keep every discovered route");
    }

    #[test]
    fn effective_seeds_is_empty_for_dynamic_games() {
        let routes = vec![addr(30, 7777), addr(31, 7777)];
        assert!(effective_seeds(true, &routes).is_empty());
        // No routes at all is also empty and harmless.
        assert!(effective_seeds(false, &[]).is_empty());
    }

    #[test]
    fn seeding_multiple_routes_locks_the_first_but_keeps_the_set() {
        let t0 = Instant::now();
        let routes = vec![addr(40, 7777), addr(41, 7777)];
        let mut t = tracker();
        t.seed(&routes, t0);
        assert_eq!(t.locked_server(), Some(routes[0]));
        assert_eq!(t.seeded_routes(), routes.as_slice());
    }

    // ── (f) zero-length / empty payload handling ───────────────────────────
    #[test]
    fn empty_payload_is_passthrough_and_never_counts() {
        let t0 = Instant::now();
        let a = addr(50, 7777);
        let mut t = tracker();

        // Empty datagrams must never advance detection toward a lock.
        for i in 0..5 {
            assert_eq!(t.observe(a, at(t0, i * 10), 0), Decision::PassThrough);
        }
        assert!(!t.is_locked());
        assert_eq!(t.candidate_count(), 0);
    }

    #[test]
    fn empty_payload_to_locked_server_does_not_refresh_staleness() {
        let t0 = Instant::now();
        let a = addr(51, 7777);
        let b = addr(52, 7777);
        let mut t = tracker();
        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // Empty payload at t0+6000 does not count as A activity...
        assert_eq!(t.observe(a, at(t0, 6000), 0), Decision::PassThrough);
        // ...so a real packet to B still sees A as stale.
        assert_eq!(t.observe(b, at(t0, 6010), 100), Decision::ResetToDetection);
    }
}
