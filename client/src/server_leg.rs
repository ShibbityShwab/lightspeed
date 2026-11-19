//! Relay-measured relay-to-server leg (R2).
//!
//! The relay is the only endpoint that can measure the second half of a relayed
//! path: it sends an ICMP echo burst to a game server a live session already
//! forwards to and reports `S = median(relay → server → relay)` over the
//! control plane (`ControlMessage::ServerLeg`). The client cannot derive that
//! leg from traffic (docs/server-leg-signal.md §4.1), so this figure is the
//! first measured server leg the product can show.
//!
//! Next to `S`, the relay also reports a **passive app-RTT sample**
//! (`ControlMessage::ServerLegApp`): the time from forwarding a game-data packet
//! to reading the first response, which the relay already tracked as a
//! timestamp difference. It adds no traffic and is frequent (one sample per
//! answered exchange), which is why the F4 design uses it as the relay-side peer
//! reference instead of the sparse client quiet-flow sample. This module stores
//! it under the same `(relay, server)` key, in its own bounded map, and exposes
//! it through its own accessor and one stable log line per accepted report.
//!
//! A relay's report is data from the network, not a fact. This module stores it
//! per **(relay, server)** pair and only exposes a report that passes every
//! validation rule below; anything else is dropped, never clamped:
//!
//! * **Physical shape** - a zero or absurd median is not a measurement, and a
//!   zero sample count is no evidence at all.
//! * **Freshness** - a report older than [`MAX_REPORT_AGE`] is hidden. The age
//!   is the relay's own staleness at send (`age_ms`) plus the time since this
//!   client received it, so a stale cached report cannot be presented as
//!   current.
//! * **The triangle floor** - a relayed path is never shorter than the direct
//!   one it replaces. When a fresh direct ICMP ping for the same server and a
//!   client→relay leg are known, `client_leg + S` must not be meaningfully
//!   below the direct ping ([`FLOOR_SLACK_PERCENT`]); a report that violates
//!   the inequality is a bad sample from an honest relay or a false one from a
//!   dishonest relay, and is refused either way. When either term is missing
//!   the floor cannot be checked: the report is still accepted (a relay that
//!   measured its leg did measure it) but it is marked
//!   [`ServerLegReport::validated`] `= false`, so no surface can present it as
//!   plainly "measured" without saying the floor did not apply.
//! * **Pair binding** - the report is stored under the relay whose control
//!   connection delivered it and the server it names. No lookup can return one
//!   relay's measurement for another relay, or one server's for another server.
//!
//! The passive sample follows the same shape and freshness rules. Its floor is
//! the design's `direct - L1` (not `client_leg + S`): a passive figure below it
//! cannot be a real relay→server round trip, so it is **marked invalid rather
//! than dropped** ([`ServerLegAppReport::floor_ok`]) - the design's F4 rule that
//! an impossible passive sample is the client term's problem, not relay error.
//!
//! Rejections fail closed: the GUI hides the row rather than showing a guess.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long a received report remains displayable.
///
/// The relay's probe cadence is 10 s and its cache TTL 30 s, so 120 s is four
/// cache lifetimes: a report this old means the relay stopped reporting, not
/// that it is slightly behind.
pub const MAX_REPORT_AGE: Duration = Duration::from_secs(120);

/// Largest median the client will believe, microseconds (5 s).
///
/// A relay→server round trip above this is not a game server anyone plays on;
/// it is an offset or a misparsed reply. The same ceiling the destination
/// estimator applies to a whole-path sample.
pub const MAX_PLAUSIBLE_MEDIAN_US: u32 = 5_000_000;

/// Below this many microseconds a "round trip" was answered locally rather than
/// over the public internet (the same physical argument as the direct-ping
/// floor, 1 ms).
pub const MIN_PLAUSIBLE_MEDIAN_US: u32 = 1_000;

/// How far `client_leg + S` may fall below the direct ping and still be
/// believed, as a percentage of the direct ping. Ten percent absorbs estimator
/// noise between two different measurements; more than that cannot be a
/// measurement error.
pub const FLOOR_SLACK_PERCENT: u64 = 10;

/// How old a direct ICMP ping may be before it stops validating a report. A
/// stale ping describes a path the client is no longer on.
const DIRECT_FLOOR_TTL: Duration = Duration::from_secs(120);

/// Upper bound on stored reports, so the map cannot grow without bound when a
/// client sees many relays and servers.
const MAX_ENTRIES: usize = 64;

/// Upper bound on stored passive reports. Its own map and its own bound: the
/// two report types arrive independently, so one must not evict the other.
const MAX_APP_ENTRIES: usize = 64;

/// A relay-measured relay-to-server leg, already validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerLegReport {
    /// Relay→server round trip, microseconds (median of the relay's echo burst).
    pub median_us: u32,
    /// Mean absolute delta between the relay's burst medians, microseconds.
    pub jitter_us: u32,
    /// How many burst medians the relay's window holds.
    pub samples: u8,
    /// Total staleness: relay-reported age at send plus time since receipt.
    pub age: Duration,
    /// Whether the triangle-floor check ran and the report passed it.
    ///
    /// `true` only when a fresh direct ICMP ping for this server and a measured
    /// client→relay leg were both available, so the inequality was actually
    /// evaluated. `false` means the report is accepted fail-open (either term was
    /// missing) and the floor could not be applied: the figure is still the
    /// relay's measurement, but a surface that shows it must not claim it was
    /// validated.
    pub validated: bool,
}

/// One stored report, before validation.
#[derive(Debug, Clone, Copy)]
struct Stored {
    median_us: u32,
    jitter_us: u32,
    samples: u8,
    /// Staleness the relay reported at send time.
    wire_age: Duration,
    /// When this client received the frame.
    received_at: Instant,
}

/// A relay-passive relay-to-server app-RTT report, already validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerLegAppReport {
    /// Median of the relay's recent forward→first-response gaps, microseconds.
    pub passive_median_us: u32,
    /// How many samples the relay's window holds.
    pub samples: u8,
    /// Total staleness: relay-reported age at send plus time since receipt.
    pub age: Duration,
    /// Whether the sample clears the triangle floor.
    ///
    /// The design's passive floor is `direct - L1` (the direct ping minus the
    /// client→relay leg), both client measurements. `false` means the floor was
    /// checkable and the passive figure fell below it: an **invalid passive
    /// sample** per the F4 rule, which is flagged rather than used as evidence
    /// of relay error. `true` means the sample clears the floor or the floor
    /// terms were absent, so it was not disproven. This is a data-quality flag,
    /// not a relay-validity verdict: the report is returned either way.
    pub floor_ok: bool,
}

/// One stored passive report, before validation.
#[derive(Debug, Clone, Copy)]
struct StoredApp {
    median_us: u32,
    samples: u8,
    /// Staleness the relay reported at send time.
    wire_age: Duration,
    /// When this client received the frame.
    received_at: Instant,
}

fn store() -> &'static Mutex<HashMap<(SocketAddrV4, Ipv4Addr), Stored>> {
    static STORE: OnceLock<Mutex<HashMap<(SocketAddrV4, Ipv4Addr), Stored>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_store() -> MutexGuard<'static, HashMap<(SocketAddrV4, Ipv4Addr), Stored>> {
    store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Record one relay-measured server-leg report received on `relay`'s control
/// connection, for `server`.
///
/// Shape validation happens here so a malformed frame is never stored; the
/// freshness and triangle-floor rules run at read time, because they depend on
/// clocks and on measurements the client may only get later. Kept crate-private:
/// the control-plane handler is the only producer, and it passes the relay of
/// the connection the frame arrived on, which is the pair-binding guarantee.
pub(crate) fn record(
    relay: SocketAddrV4,
    server: Ipv4Addr,
    median_us: u32,
    jitter_us: u32,
    samples: u8,
    age_ms: u16,
) {
    record_at(
        relay,
        server,
        median_us,
        jitter_us,
        samples,
        Duration::from_millis(u64::from(age_ms)),
        Instant::now(),
    );
}

/// [`record`] with the receive instant supplied, so the freshness rules are
/// testable without sleeping.
fn record_at(
    relay: SocketAddrV4,
    server: Ipv4Addr,
    median_us: u32,
    jitter_us: u32,
    samples: u8,
    wire_age: Duration,
    received_at: Instant,
) {
    if !is_plausible_median(median_us) || !is_plausible_jitter(jitter_us) || samples == 0 {
        return;
    }
    let stored = Stored {
        median_us,
        jitter_us,
        samples,
        wire_age,
        received_at,
    };
    let mut entries = lock_store();
    if entries.len() >= MAX_ENTRIES && !entries.contains_key(&(relay, server)) {
        // Evict the stalest entry rather than growing without bound. A stale
        // entry is already unusable, so nothing displayable is lost.
        if let Some(stalest) = entries
            .iter()
            .min_by_key(|(_, value)| value.received_at)
            .map(|(key, _)| *key)
        {
            entries.remove(&stalest);
        }
    }
    entries.insert((relay, server), stored);
}

/// The latest relay-measured server leg for `(relay, server)`.
///
/// `None` when no report was received for this exact pair, when the report is
/// older than [`MAX_REPORT_AGE`], when it is physically implausible, or when
/// `client_leg + S` is meaningfully below the freshest direct ping for the same
/// server. When a fresh direct ping or the client leg is missing the report is
/// still returned (the relay did measure its leg), with
/// [`ServerLegReport::validated`] `= false`; the caller must qualify it. The
/// relay used is the one whose control connection delivered the report; a
/// report from another relay is stored under that relay and is never returned
/// here.
pub fn server_leg_report(relay: SocketAddrV4, server: Ipv4Addr) -> Option<ServerLegReport> {
    let now = Instant::now();
    let stored = *lock_store().get(&(relay, server))?;
    let client_leg_us = fresh_client_leg_us(relay);
    let direct_us = fresh_direct_us(server);
    validate(stored, now, client_leg_us, direct_us)
}

/// The freshest client→relay leg for `relay`, microseconds.
///
/// The keepalive p50 is the one-RTT client leg the route surface shows as
/// `Relay RTT`; a non-finite or zero reading is not a measurement.
fn fresh_client_leg_us(relay: SocketAddrV4) -> Option<u64> {
    crate::latency::client_relay_p50_ms(relay)
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .map(|ms| (f64::from(ms) * 1000.0).round() as u64)
}

/// The freshest direct ICMP ping for `server`, only while it is fresh enough to
/// be physical evidence about the current path.
fn fresh_direct_us(server: Ipv4Addr) -> Option<u64> {
    let (ms, age) = crate::latency::direct_sample(server)?;
    if age > DIRECT_FLOOR_TTL || !ms.is_finite() || ms <= 0.0 {
        return None;
    }
    Some((f64::from(ms) * 1000.0).round() as u64)
}

/// Validate one stored report at `now` against the client leg and direct ping.
///
/// Split out from [`server_leg_report`] so every rule is testable with scripted
/// instants and measurements instead of the process-wide stores.
fn validate(
    stored: Stored,
    now: Instant,
    client_leg_us: Option<u64>,
    direct_us: Option<u64>,
) -> Option<ServerLegReport> {
    if !is_plausible_median(stored.median_us) || !is_plausible_jitter(stored.jitter_us) {
        return None;
    }
    if stored.samples == 0 {
        return None;
    }
    let age = stored
        .wire_age
        .saturating_add(now.saturating_duration_since(stored.received_at));
    if age > MAX_REPORT_AGE {
        return None;
    }
    // The triangle floor: a relayed path cannot be shorter than the direct
    // path it replaces. Only checkable while both terms exist; a missing
    // client leg or direct ping is "not measured", not "failed", so the report
    // is still accepted - fail-open - but marked unvalidated.
    let validated = client_leg_us.is_some() && direct_us.is_some();
    if let (Some(client_leg_us), Some(direct_us)) = (client_leg_us, direct_us) {
        let total_us = client_leg_us.saturating_add(u64::from(stored.median_us));
        let floor_us = direct_us
            .saturating_mul(100 - FLOOR_SLACK_PERCENT)
            .saturating_div(100);
        if total_us < floor_us {
            return None;
        }
    }
    Some(ServerLegReport {
        median_us: stored.median_us,
        jitter_us: stored.jitter_us,
        samples: stored.samples,
        age,
        validated,
    })
}

fn is_plausible_median(median_us: u32) -> bool {
    (MIN_PLAUSIBLE_MEDIAN_US..=MAX_PLAUSIBLE_MEDIAN_US).contains(&median_us)
}

/// A jitter above the median ceiling is not a spread, it is a bad field.
fn is_plausible_jitter(jitter_us: u32) -> bool {
    jitter_us <= MAX_PLAUSIBLE_MEDIAN_US
}

// ── Passive app-RTT reports (F4 peer reference) ─────────────────────

fn app_store() -> &'static Mutex<HashMap<(SocketAddrV4, Ipv4Addr), StoredApp>> {
    static STORE: OnceLock<Mutex<HashMap<(SocketAddrV4, Ipv4Addr), StoredApp>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_app_store() -> MutexGuard<'static, HashMap<(SocketAddrV4, Ipv4Addr), StoredApp>> {
    app_store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Record one relay-passive app-RTT report received on `relay`'s control
/// connection, for `server`.
///
/// Shape validation happens here so a zero or absurd frame is never stored;
/// freshness and the floor flag run at read time because they depend on clocks
/// and client measurements that may arrive later. Returns `true` when the report
/// was stored: the control-plane handler emits exactly one info line per
/// accepted report, so a rejected frame must not log one. Kept crate-private:
/// the control-plane handler is the only producer, and it passes the relay of
/// the connection the frame arrived on, which is the pair-binding guarantee.
pub(crate) fn record_app(
    relay: SocketAddrV4,
    server: Ipv4Addr,
    median_us: u32,
    samples: u8,
    age_ms: u16,
) -> bool {
    record_app_at(
        relay,
        server,
        median_us,
        samples,
        Duration::from_millis(u64::from(age_ms)),
        Instant::now(),
    )
}

/// [`record_app`] with the receive instant supplied, so the store is testable
/// without sleeping.
fn record_app_at(
    relay: SocketAddrV4,
    server: Ipv4Addr,
    median_us: u32,
    samples: u8,
    wire_age: Duration,
    received_at: Instant,
) -> bool {
    if !is_plausible_median(median_us) || samples == 0 {
        return false;
    }
    let stored = StoredApp {
        median_us,
        samples,
        wire_age,
        received_at,
    };
    let mut entries = lock_app_store();
    if entries.len() >= MAX_APP_ENTRIES && !entries.contains_key(&(relay, server)) {
        // Same eviction rule as the ICMP store: drop the stalest entry, which
        // is already unusable, rather than growing without bound.
        if let Some(stalest) = entries
            .iter()
            .min_by_key(|(_, value)| value.received_at)
            .map(|(key, _)| *key)
        {
            entries.remove(&stalest);
        }
    }
    entries.insert((relay, server), stored);
    true
}

/// The latest passive relay→server app-RTT report for `(relay, server)`.
///
/// `None` when no report was received for this exact pair, when it is older
/// than [`MAX_REPORT_AGE`], or when its median/sample count is physically
/// implausible. A report whose passive median is below the design's
/// `direct - client_leg` floor is **still returned**, with
/// [`ServerLegAppReport::floor_ok`] `= false`; the F4 rule flags that as an
/// invalid passive sample instead of discarding it or blaming the relay. The
/// relay is the one whose control connection delivered the frame; another
/// relay's report is never returned here.
pub fn server_leg_app_report(relay: SocketAddrV4, server: Ipv4Addr) -> Option<ServerLegAppReport> {
    let now = Instant::now();
    let stored = *lock_app_store().get(&(relay, server))?;
    let client_leg_us = fresh_client_leg_us(relay);
    let direct_us = fresh_direct_us(server);
    validate_app(stored, now, client_leg_us, direct_us)
}

/// Validate one stored passive report at `now`, applying the freshness rule and
/// the `direct - L1` floor flag.
///
/// Split out from [`server_leg_app_report`] so every rule is testable with
/// scripted instants and measurements instead of the process-wide stores.
fn validate_app(
    stored: StoredApp,
    now: Instant,
    client_leg_us: Option<u64>,
    direct_us: Option<u64>,
) -> Option<ServerLegAppReport> {
    if !is_plausible_median(stored.median_us) || stored.samples == 0 {
        return None;
    }
    let age = stored
        .wire_age
        .saturating_add(now.saturating_duration_since(stored.received_at));
    if age > MAX_REPORT_AGE {
        return None;
    }
    // The design's F4 floor: `direct - L1`. Unlike the ICMP report, a violation
    // does not invalidate the report as a measurement; it marks the sample as
    // physically impossible for the current client measurements, which is what
    // the harvest must flag rather than treat as relay error. Missing terms
    // cannot disprove anything, so the flag stays clear.
    let floor_ok = match (client_leg_us, direct_us) {
        (Some(client_leg_us), Some(direct_us)) => {
            let floor_us = direct_us.saturating_sub(client_leg_us);
            u64::from(stored.median_us) >= floor_us
        }
        _ => true,
    };
    Some(ServerLegAppReport {
        passive_median_us: stored.median_us,
        samples: stored.samples,
        age,
        floor_ok,
    })
}

/// The one stable info line emitted per accepted passive report.
///
/// Format (pinned for the E1 harvest; do not reorder or rename fields):
///
/// ```text
/// SERVER_LEG_APP relay=<ip:port> server=<ip> passive_us=<u64> samples=<u8> age_ms=<u16> at_unix_ms=<u128>
/// ```
///
/// `server` is the wire's IPv4 destination only: neither report message carries
/// a port, so none is invented. `relay` is the control connection's address and
/// `at_unix_ms` the client's receipt time, both needed to match a passive sample
/// to the relay's other measurements in the F4 epoch.
pub fn server_leg_app_log_line(
    relay: SocketAddrV4,
    server: Ipv4Addr,
    passive_us: u64,
    samples: u8,
    age_ms: u16,
    at_unix_ms: u128,
) -> String {
    format!(
        "SERVER_LEG_APP relay={relay} server={server} passive_us={passive_us} \
         samples={samples} age_ms={age_ms} at_unix_ms={at_unix_ms}"
    )
}

/// Drop every stored passive report. Tests only.
#[cfg(test)]
fn clear_app() {
    lock_app_store().clear();
}

/// Drop every stored report. Tests only: the process-global map would otherwise
/// leak state between test threads.
#[cfg(test)]
fn clear() {
    lock_store().clear();
}

/// The test harness runs `#[test]`s in parallel and the report store is
/// process-global, so tests that mutate it take this lock to stay
/// deterministic (the same pattern as `session::token_test_guard`).
#[cfg(test)]
pub(crate) fn store_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static STORE_TEST_LOCK: Mutex<()> = Mutex::new(());
    STORE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay(port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), port)
    }

    fn server(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(91, 199, 82, last)
    }

    fn stored(
        median_us: u32,
        jitter_us: u32,
        samples: u8,
        wire_age: Duration,
        received_at: Instant,
    ) -> Stored {
        Stored {
            median_us,
            jitter_us,
            samples,
            wire_age,
            received_at,
        }
    }

    fn stored_app(
        median_us: u32,
        samples: u8,
        wire_age: Duration,
        received_at: Instant,
    ) -> StoredApp {
        StoredApp {
            median_us,
            samples,
            wire_age,
            received_at,
        }
    }

    #[test]
    fn a_plausible_report_is_accepted_with_its_metadata() {
        let now = Instant::now();
        // client leg 150 ms + relay leg 148 ms = 298 ms against a 300 ms direct
        // ping: a relayed path that close to direct is physically possible.
        let report = validate(
            stored(148_000, 3_500, 4, Duration::from_millis(1_200), now),
            now,
            Some(150_000),
            Some(300_000),
        )
        .expect("a plausible report must be accepted");
        assert_eq!(report.median_us, 148_000);
        assert_eq!(report.jitter_us, 3_500);
        assert_eq!(report.samples, 4);
        assert_eq!(report.age, Duration::from_millis(1_200));
        assert!(
            report.validated,
            "both floor terms were present, so the floor check ran and passed"
        );
    }

    #[test]
    fn a_report_without_the_floor_terms_is_accepted_but_unvalidated() {
        let now = Instant::now();
        let entry = stored(40_000, 500, 3, Duration::ZERO, now);

        // No direct ping: accepted fail-open, but the floor never ran.
        let no_direct = validate(entry, now, Some(50_000), None).expect("accepted fail-open");
        assert!(!no_direct.validated);
        // No client leg: same.
        let no_client = validate(entry, now, None, Some(300_000)).expect("accepted fail-open");
        assert!(!no_client.validated);
        // Neither term: still the relay's measurement, still unvalidated.
        let neither = validate(entry, now, None, None).expect("accepted fail-open");
        assert!(!neither.validated);

        // Both terms present and the floor passes: validated.
        let checked = validate(
            stored(220_000, 500, 3, Duration::ZERO, now),
            now,
            Some(50_000),
            Some(300_000),
        )
        .expect("at the slack boundary the floor passes");
        assert!(checked.validated);
    }

    #[test]
    fn the_age_includes_time_since_receipt() {
        let now = Instant::now();
        let report = validate(
            stored(
                90_000,
                0,
                3,
                Duration::from_millis(500),
                now - Duration::from_secs(9),
            ),
            now,
            None,
            None,
        )
        .expect("a fresh report is accepted");
        assert_eq!(report.age, Duration::from_micros(9_500_000));
    }

    #[test]
    fn zero_and_absurd_medians_are_rejected() {
        let now = Instant::now();
        for median_us in [0, 999, MAX_PLAUSIBLE_MEDIAN_US + 1, u32::MAX] {
            assert!(
                validate(
                    stored(median_us, 0, 3, Duration::ZERO, now),
                    now,
                    None,
                    None
                )
                .is_none(),
                "median {median_us} µs must be rejected"
            );
        }
        // A zero sample count is no evidence, even with a plausible median.
        assert!(validate(stored(50_000, 0, 0, Duration::ZERO, now), now, None, None).is_none());
    }

    #[test]
    fn a_report_older_than_the_limit_is_rejected() {
        let now = Instant::now();
        // Relay-reported age alone already exceeds the limit.
        assert!(validate(
            stored(50_000, 0, 3, MAX_REPORT_AGE + Duration::from_secs(1), now),
            now,
            None,
            None,
        )
        .is_none());
        // A stale receive time adds up with the wire age.
        assert!(validate(
            stored(50_000, 0, 3, Duration::from_secs(30), now - MAX_REPORT_AGE),
            now,
            None,
            None,
        )
        .is_none());
        // Just inside the limit is still displayable.
        assert!(validate(
            stored(
                50_000,
                0,
                3,
                Duration::from_secs(30),
                now - (MAX_REPORT_AGE - Duration::from_secs(31))
            ),
            now,
            None,
            None,
        )
        .is_some());
    }

    #[test]
    fn a_total_meaningfully_below_the_direct_ping_is_rejected() {
        let now = Instant::now();
        // client leg 50 ms + relay leg 40 ms = 90 ms against a 300 ms direct
        // ping: physically impossible for a relayed path.
        assert!(validate(
            stored(40_000, 500, 3, Duration::ZERO, now),
            now,
            Some(50_000),
            Some(300_000),
        )
        .is_none());
        // Exactly at the 10 % slack boundary is accepted: 270 ms total against
        // a 300 ms direct ping, with the floor check therefore validated.
        let at_boundary = validate(
            stored(220_000, 500, 3, Duration::ZERO, now),
            now,
            Some(50_000),
            Some(300_000),
        )
        .expect("the boundary is accepted");
        assert!(at_boundary.validated);
        // Without a direct ping there is no floor to fail: accepted, but the
        // report is explicitly unvalidated.
        let fail_open = validate(
            stored(40_000, 500, 3, Duration::ZERO, now),
            now,
            Some(50_000),
            None,
        )
        .expect("fail-open acceptance");
        assert!(!fail_open.validated);
    }

    #[test]
    fn a_report_is_bound_to_its_relay_and_server_pair() {
        let _guard = store_test_guard();
        let now = Instant::now();
        let entry = stored(120_000, 1_000, 3, Duration::ZERO, now);
        // The validation itself is pair-agnostic; the store key is what binds
        // it. Prove the whole path: record for (relay A, server X) and demand
        // every other pair.
        clear();
        record_at(
            relay(4433),
            server(10),
            120_000,
            1_000,
            3,
            Duration::ZERO,
            now,
        );
        assert!(validate(entry, now, None, None).is_some());
        assert!(store()
            .lock()
            .expect("store")
            .contains_key(&(relay(4433), server(10))));
        assert!(
            !store()
                .lock()
                .expect("store")
                .contains_key(&(relay(4434), server(10))),
            "another relay must not see this measurement"
        );
        assert!(
            !store()
                .lock()
                .expect("store")
                .contains_key(&(relay(4433), server(11))),
            "another server must not see this measurement"
        );
        clear();
    }

    #[test]
    fn record_rejects_malformed_frames_and_keeps_the_last_good_report() {
        let _guard = store_test_guard();
        clear();
        let now = Instant::now();
        record_at(
            relay(4433),
            server(10),
            100_000,
            2_000,
            3,
            Duration::ZERO,
            now,
        );
        // A zero median, an absurd median, a bad jitter and zero samples are
        // all refused, so the good entry stays.
        record_at(relay(4433), server(10), 0, 2_000, 3, Duration::ZERO, now);
        record_at(
            relay(4433),
            server(10),
            u32::MAX,
            2_000,
            3,
            Duration::ZERO,
            now,
        );
        record_at(
            relay(4433),
            server(10),
            100_000,
            u32::MAX,
            3,
            Duration::ZERO,
            now,
        );
        record_at(
            relay(4433),
            server(10),
            100_000,
            2_000,
            0,
            Duration::ZERO,
            now,
        );
        let held = *store()
            .lock()
            .expect("store")
            .get(&(relay(4433), server(10)))
            .expect("the good report survived");
        assert_eq!(held.median_us, 100_000);
        assert_eq!(held.jitter_us, 2_000);
        clear();
    }

    #[test]
    fn the_store_is_bounded() {
        let _guard = store_test_guard();
        clear();
        let now = Instant::now();
        for port in 0..(MAX_ENTRIES as u16 + 8) {
            record_at(
                relay(1_000 + port),
                server(1),
                100_000,
                1_000,
                3,
                Duration::ZERO,
                now,
            );
        }
        assert!(store().lock().expect("store").len() <= MAX_ENTRIES);
        clear();
    }

    #[test]
    fn a_foreign_relay_report_cannot_serve_the_active_pair() {
        // The accessor's inputs are the store key and the process-wide direct
        // measurements; with no reports for the active relay the honest answer
        // is `None`, never another relay's value.
        let _guard = store_test_guard();
        clear();
        record_at(
            relay(4433),
            server(10),
            90_000,
            1_000,
            3,
            Duration::ZERO,
            Instant::now(),
        );
        assert!(server_leg_report(relay(4434), server(10)).is_none());
        assert!(server_leg_report(relay(4433), server(11)).is_none());
        assert!(server_leg_report(relay(4433), server(10)).is_some());
        clear();
    }

    // ── Passive app-RTT reports (F4 peer reference) ─────────────────

    /// Neutral TEST-NET fixtures for the passive path; never a real address.
    fn testnet_relay(port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), port)
    }

    fn testnet_server(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(198, 51, 100, last)
    }

    #[test]
    fn a_plausible_passive_report_is_accepted_with_its_metadata() {
        let now = Instant::now();
        // direct 300 ms - client leg 150 ms = a 150 ms floor; 160 ms clears it.
        let report = validate_app(
            stored_app(160_000, 6, Duration::from_millis(1_200), now),
            now,
            Some(150_000),
            Some(300_000),
        )
        .expect("a plausible passive report must be accepted");
        assert_eq!(report.passive_median_us, 160_000);
        assert_eq!(report.samples, 6);
        assert_eq!(report.age, Duration::from_millis(1_200));
        assert!(report.floor_ok, "160 ms clears the 150 ms floor");
    }

    #[test]
    fn a_passive_below_the_direct_minus_client_leg_floor_is_flagged_not_dropped() {
        let now = Instant::now();
        let entry = stored_app(40_000, 3, Duration::ZERO, now);
        // direct 300 ms - client leg 50 ms = 250 ms floor; 40 ms is impossible,
        // so the sample is marked invalid - and still returned.
        let below = validate_app(entry, now, Some(50_000), Some(300_000))
            .expect("an invalid passive sample is flagged, not dropped");
        assert!(!below.floor_ok);
        assert_eq!(below.passive_median_us, 40_000);

        // Exactly at the floor is allowed.
        let at_floor = validate_app(
            stored_app(250_000, 3, Duration::ZERO, now),
            now,
            Some(50_000),
            Some(300_000),
        )
        .expect("the boundary clears the floor");
        assert!(at_floor.floor_ok);

        // Without both terms the floor cannot be applied: not disproven.
        for (client_leg_us, direct_us) in
            [(Some(50_000), None), (None, Some(300_000)), (None, None)]
        {
            let unchecked = validate_app(entry, now, client_leg_us, direct_us)
                .expect("a missing floor term is not a rejection");
            assert!(unchecked.floor_ok, "an unchecked floor cannot be flagged");
        }
    }

    #[test]
    fn zero_and_absurd_passive_medians_are_rejected() {
        let now = Instant::now();
        for median_us in [0, 999, MAX_PLAUSIBLE_MEDIAN_US + 1, u32::MAX] {
            assert!(
                validate_app(
                    stored_app(median_us, 3, Duration::ZERO, now),
                    now,
                    None,
                    None
                )
                .is_none(),
                "passive median {median_us} µs must be rejected"
            );
        }
        // A zero sample count is no evidence, even with a plausible median.
        assert!(
            validate_app(stored_app(50_000, 0, Duration::ZERO, now), now, None, None).is_none()
        );
    }

    #[test]
    fn a_passive_report_older_than_the_limit_is_rejected() {
        let now = Instant::now();
        // Relay-reported age alone already exceeds the limit.
        assert!(validate_app(
            stored_app(50_000, 3, MAX_REPORT_AGE + Duration::from_secs(1), now),
            now,
            None,
            None,
        )
        .is_none());
        // A stale receive time adds up with the wire age.
        assert!(validate_app(
            stored_app(50_000, 3, Duration::from_secs(30), now - MAX_REPORT_AGE),
            now,
            None,
            None,
        )
        .is_none());
        // Just inside the limit is still displayable.
        assert!(validate_app(
            stored_app(
                50_000,
                3,
                Duration::from_secs(30),
                now - (MAX_REPORT_AGE - Duration::from_secs(31))
            ),
            now,
            None,
            None,
        )
        .is_some());
    }

    #[test]
    fn a_passive_report_is_bound_to_its_relay_and_server_pair() {
        let _guard = store_test_guard();
        clear_app();
        let now = Instant::now();
        assert!(record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            120_000,
            4,
            Duration::ZERO,
            now,
        ));
        assert!(lock_app_store().contains_key(&(testnet_relay(4433), testnet_server(10))));
        assert!(
            !lock_app_store().contains_key(&(testnet_relay(4434), testnet_server(10))),
            "another relay must not see this passive report"
        );
        assert!(
            !lock_app_store().contains_key(&(testnet_relay(4433), testnet_server(11))),
            "another server must not see this passive report"
        );
        clear_app();
    }

    #[test]
    fn record_app_rejects_malformed_frames_and_keeps_the_last_good_report() {
        let _guard = store_test_guard();
        clear_app();
        let now = Instant::now();
        assert!(record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            100_000,
            3,
            Duration::ZERO,
            now,
        ));
        // A zero median, an absurd median and zero samples are refused (the
        // status is what gates the one info line), so the good entry stays.
        assert!(!record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            0,
            3,
            Duration::ZERO,
            now,
        ));
        assert!(!record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            u32::MAX,
            3,
            Duration::ZERO,
            now,
        ));
        assert!(!record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            100_000,
            0,
            Duration::ZERO,
            now,
        ));
        let held = *lock_app_store()
            .get(&(testnet_relay(4433), testnet_server(10)))
            .expect("the good report survived");
        assert_eq!(held.median_us, 100_000);
        assert_eq!(held.samples, 3);
        clear_app();
    }

    #[test]
    fn the_passive_store_is_bounded() {
        let _guard = store_test_guard();
        clear_app();
        let now = Instant::now();
        for port in 0..(MAX_APP_ENTRIES as u16 + 8) {
            record_app_at(
                testnet_relay(1_000 + port),
                testnet_server(1),
                100_000,
                3,
                Duration::ZERO,
                now,
            );
        }
        assert!(lock_app_store().len() <= MAX_APP_ENTRIES);
        clear_app();
    }

    #[test]
    fn a_foreign_passive_report_cannot_serve_the_active_pair() {
        // The accessor's inputs are the store key and the process-wide client
        // measurements; with no report for the active pair the honest answer is
        // `None`, never another pair's value.
        let _guard = store_test_guard();
        clear_app();
        record_app_at(
            testnet_relay(4433),
            testnet_server(10),
            90_000,
            3,
            Duration::ZERO,
            Instant::now(),
        );
        assert!(server_leg_app_report(testnet_relay(4434), testnet_server(10)).is_none());
        assert!(server_leg_app_report(testnet_relay(4433), testnet_server(11)).is_none());
        assert!(server_leg_app_report(testnet_relay(4433), testnet_server(10)).is_some());
        clear_app();
    }

    #[test]
    fn the_passive_log_line_round_trips() {
        // TEST-NET-3 / TEST-NET-2 fixtures only: the line must be reproducible
        // from neutral addresses, never a real game server.
        let relay = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 4433);
        let server = Ipv4Addr::new(198, 51, 100, 23);
        let line = server_leg_app_log_line(relay, server, 47_250, 12, 900, 1_765_000_000_123);
        assert_eq!(
            line,
            "SERVER_LEG_APP relay=203.0.113.7:4433 server=198.51.100.23 \
             passive_us=47250 samples=12 age_ms=900 at_unix_ms=1765000000123"
        );

        // Round-trip: the whitespace-separated field list decodes back to the
        // exact inputs, so a reordered or renamed field fails here.
        let mut fields = std::collections::HashMap::new();
        let mut parts = line.split_whitespace();
        assert_eq!(parts.next(), Some("SERVER_LEG_APP"));
        for part in parts {
            let (key, value) = part.split_once('=').expect("key=value field");
            fields.insert(key, value);
        }
        assert_eq!(fields.get("relay"), Some(&"203.0.113.7:4433"));
        assert_eq!(fields.get("server"), Some(&"198.51.100.23"));
        assert_eq!(fields.get("passive_us"), Some(&"47250"));
        assert_eq!(fields.get("samples"), Some(&"12"));
        assert_eq!(fields.get("age_ms"), Some(&"900"));
        assert_eq!(fields.get("at_unix_ms"), Some(&"1765000000123"));
        assert_eq!(fields.len(), 6, "exactly the pinned fields");
    }
}
