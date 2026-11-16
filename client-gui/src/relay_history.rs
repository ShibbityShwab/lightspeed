//! What each relay actually did for each game server.
//!
//! The relayed round trip cannot be measured from the client: a UDP game gives no
//! per-packet correlation between a send and a reply, and three estimators built
//! on that assumption produced numbers that were physically impossible. What can
//! be measured exactly is an *aggregate*: of the packets this client pushed
//! through a relay for a given server, how many replies came back through it. A
//! ratio needs no attribution, so it cannot be wrong the way a paired sample can.
//!
//! That is the signal this module remembers, per (relay, server), across sessions
//! and across restarts. It answers the question the relay chooser actually faces -
//! "has this relay ever carried this server's traffic?" - without inventing a
//! latency figure nobody can measure.
//!
//! Two kinds of knowledge live here, and they are not interchangeable:
//!
//! - **A measurement**, accumulated from the interceptor's own per-server
//!   counters. It can say a relay carries the server (enough traffic, at least one
//!   reply in eight). It cannot condemn a relay: game UDP traffic is asymmetric,
//!   and a working relay measured a 24.1% return share on a live run.
//! - **A verdict**, written only after the no-reply watchdog spent its re-tests on
//!   a confirmed zero-return chain. That one may exclude the relay for
//!   [`VERDICT_TTL`].

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Denominator of the return share that counts as a working return path: one
/// datagram in eight.
///
/// Game UDP traffic is asymmetric - the client sends far more than the server
/// returns - so the question is not "which relay returns the most" but "does
/// anything come back at all". A live healthy relay returned 13 datagrams for 54
/// intercepted (24.1%) while the relayed path was 150 ms faster than direct, and
/// the old quarter-share bar called it starved. The no-reply watchdog's fault
/// floor and this module's "carries" bar are the same fact, so they share this
/// one constant and can never disagree about what "carrying" means.
pub const RETURN_SHARE_DENOMINATOR: u64 = 8;

/// The same floor as a fraction, for the share arithmetic below.
const CARRIES_SHARE: f64 = 1.0 / RETURN_SHARE_DENOMINATOR as f64;

/// Forwards needed before the share means anything. A session that has pushed
/// twenty packets and seen one reply is a quiet match, not a broken relay.
pub const MIN_FORWARDS: u64 = 40;

/// How recent a measurement has to be to outrank a watchdog window verdict. The
/// recorder writes every 30 s, so two minutes is recent by construction.
const CORROBORATION_WINDOW: Duration = Duration::from_secs(120);

/// How long a verdict is remembered. A relay's route to a server is stable for
/// hours, not days, and a stale verdict must not outlive the conditions.
pub const VERDICT_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// How many (relay, server) records are kept. Bounded so a long-lived install
/// cannot grow this file without limit.
pub const MAX_ENTRIES: usize = 256;

/// One (relay, server) record: the counters as last observed, and whether the
/// watchdog recorded a verdict against the relay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// Packets this client pushed through the relay for the server.
    pub forwards: u64,
    /// Replies that came back through the same relay for the same server.
    pub replies: u64,
    /// Unix seconds when the counters were last observed.
    pub observed_at: u64,
    /// Set only by [`RelayHistory::record_failure`]: a confirmed zero-return
    /// chain. A measured ratio never sets it - a low share is evidence about a
    /// window, not a verdict about the relay.
    pub failed: bool,
}

impl Record {
    pub fn answered_share(&self) -> f64 {
        if self.forwards == 0 {
            return 0.0;
        }
        (self.replies as f64 / self.forwards as f64).clamp(0.0, 1.0)
    }

    /// Whether this record's own counters show the relay carrying the server:
    /// enough traffic to judge, and at least one reply in eight coming back.
    pub fn carries(&self) -> bool {
        self.forwards >= MIN_FORWARDS && self.answered_share() >= CARRIES_SHARE
    }

    /// Whether the watchdog recorded this relay as not carrying this server, and
    /// the verdict is still recent.
    ///
    /// No share is compared here on purpose. The share already decided the
    /// watchdog's window; what is remembered is its verdict, and only a chain
    /// that returned nothing at all produces one. Deriving a second verdict from
    /// the raw ratio is how one low-share window excluded a healthy relay for
    /// six hours on a live run.
    pub fn is_starved(&self, now: u64) -> bool {
        self.failed && now.saturating_sub(self.observed_at) <= VERDICT_TTL.as_secs()
    }
}

/// Per-(relay, server) memory, keyed by `"relay_ip:port|server_ip"`.
#[derive(Debug, Default)]
pub struct RelayHistory {
    entries: HashMap<String, Record>,
}

fn key(relay: SocketAddrV4, server: Ipv4Addr) -> String {
    format!("{}|{}", relay, server)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

impl RelayHistory {
    /// Add this poll's deltas to the record for the pair.
    ///
    /// Deltas, not totals: the engine's counters are cumulative for the
    /// interceptor, so adding totals would weight old traffic as though it were
    /// happening now, and a relay that failed once would stay failed forever.
    ///
    /// A measurement can only redeem a recorded verdict, never create one: real
    /// traffic that demonstrably comes back clears `failed`. A low share on
    /// either side changes nothing but the record's numbers.
    pub fn record(&mut self, relay: SocketAddrV4, server: Ipv4Addr, forwards: u64, replies: u64) {
        if forwards == 0 && replies == 0 {
            return;
        }
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = entry.forwards.saturating_add(forwards);
        entry.replies = entry.replies.saturating_add(replies);
        entry.observed_at = now_secs();
        if entry.carries() {
            entry.failed = false;
        }
        self.trim();
    }

    /// Record the watchdog's verdict for the pair: a confirmed zero-return chain.
    ///
    /// This is the only way a relay is remembered as not carrying a server. The
    /// caller writes it once the watchdog has spent its re-tests and only when
    /// every window of that chain saw `from_relay == 0` and `injected == 0`; a
    /// low ratio, or a window with any injection, is not a verdict.
    pub fn record_failure(&mut self, relay: SocketAddrV4, server: Ipv4Addr) {
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = entry.forwards.max(MIN_FORWARDS);
        entry.replies = 0;
        entry.observed_at = now_secs();
        entry.failed = true;
        self.trim();
    }

    /// Whether the measured record for this pair says, within the last two
    /// minutes, that the relay is carrying the server.
    ///
    /// The watchdog judges short windows, and a short window starves on sparse
    /// traffic that a longer measurement shows returning normally. When that
    /// longer measurement is fresh it outranks the window: MAD was recorded as a
    /// failed relay for a server it was in fact returning 112% of the traffic for,
    /// and the exclusion that followed cost a relay that worked.
    pub fn carries(&self, relay: SocketAddrV4, server: Ipv4Addr, now: u64) -> bool {
        self.entries
            .get(&format!("{relay}|{server}"))
            .is_some_and(|record| {
                record.carries()
                    && now.saturating_sub(record.observed_at) <= CORROBORATION_WINDOW.as_secs()
            })
    }

    /// How many distinct relays are on record as not carrying this server.
    ///
    /// The chooser uses this to stop cycling: with eight candidates and two minutes
    /// of retries each, a server no relay carries costs the better part of a match
    /// in switching. Three condemnations is the point where the measurements agree
    /// with each other and further attempts are just motion.
    pub fn condemned_count(&self, server: Ipv4Addr) -> usize {
        let suffix = format!("|{server}");
        self.entries
            .iter()
            .filter(|(key, record)| key.ends_with(&suffix) && record.is_starved(now_secs()))
            .count()
    }

    /// Whether this relay is on record as not carrying this server.
    pub fn is_starved(&self, relay: SocketAddrV4, server: Ipv4Addr) -> bool {
        self.entries
            .get(&key(relay, server))
            .is_some_and(|record| record.is_starved(now_secs()))
    }

    #[cfg(test)]
    pub fn get(&self, relay: SocketAddrV4, server: Ipv4Addr) -> Option<&Record> {
        self.entries.get(&key(relay, server))
    }

    /// Drop expired verdicts, then the oldest records past the cap.
    fn trim(&mut self) {
        let now = now_secs();
        self.entries
            .retain(|_, record| now.saturating_sub(record.observed_at) <= VERDICT_TTL.as_secs());
        while self.entries.len() > MAX_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, record)| record.observed_at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    /// Load from `path`, treating a missing or unreadable file as empty.
    ///
    /// Never fails the caller: this is a convenience memory, and a corrupt file
    /// must not stop the client from starting. A record written before the
    /// `failed` flag existed loads as a measurement only, so a verdict derived by
    /// the old buggy criterion is dropped instead of excluding a working relay.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Self::default();
        };
        let mut history = Self::default();
        if let Some(entries) = value.get("entries").and_then(|v| v.as_object()) {
            for (key, record) in entries {
                let number = |name: &str| record.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
                history.entries.insert(
                    key.clone(),
                    Record {
                        forwards: number("forwards"),
                        replies: number("replies"),
                        observed_at: number("observed_at"),
                        failed: record
                            .get("failed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    },
                );
            }
        }
        history.trim();
        history
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let entries: serde_json::Map<String, serde_json::Value> = self
            .entries
            .iter()
            .map(|(key, record)| {
                (
                    key.clone(),
                    serde_json::json!({
                        "forwards": record.forwards,
                        "replies": record.replies,
                        "observed_at": record.observed_at,
                        "failed": record.failed,
                    }),
                )
            })
            .collect();
        let document = serde_json::json!({ "entries": entries });
        std::fs::write(path, serde_json::to_string_pretty(&document)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 4434)
    }

    fn server() -> Ipv4Addr {
        Ipv4Addr::new(198, 51, 100, 7)
    }

    #[test]
    fn a_measured_share_never_condemns_a_relay() {
        let mut history = RelayHistory::default();
        // The live EWR record: 93 forwards, 11 replies (11.8%) accumulated over a
        // session whose windows were returning 24.1%. Deriving a verdict from
        // that ratio excluded a working relay for six hours.
        history.record(relay(), server(), 93, 11);
        assert!(
            !history.is_starved(relay(), server()),
            "a measured share is evidence about a window, not a verdict about the relay"
        );
        let mut history = RelayHistory::default();
        // The healthy live window: 13 replies for 54 forwards (24.1%) while the
        // relayed path was 150 ms faster than direct.
        history.record(relay(), server(), 54, 13);
        assert!(!history.is_starved(relay(), server()));
        let mut history = RelayHistory::default();
        // Even a nearly silent measurement cannot condemn on its own: only the
        // watchdog's confirmed zero-return chain may.
        history.record(relay(), server(), 100, 2);
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn a_confirmed_zero_return_chain_is_a_verdict() {
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 179, 0);
        assert!(
            !history.is_starved(relay(), server()),
            "a zero-return window is not a confirmed chain until the watchdog says so"
        );
        history.record_failure(relay(), server());
        assert!(
            history.is_starved(relay(), server()),
            "the confirmed chain is the one verdict this module may store"
        );
    }

    #[test]
    fn a_healthy_relay_is_not_marked() {
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 100, 80);
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn a_thin_session_is_not_a_verdict() {
        let mut history = RelayHistory::default();
        // 10 forwards with no replies is a quiet match, not a broken relay.
        history.record(relay(), server(), 10, 0);
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn records_accumulate_across_polls() {
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 30, 1);
        history.record(relay(), server(), 30, 1);
        let record = history.get(relay(), server()).expect("recorded");
        assert_eq!(record.forwards, 60);
        assert_eq!(record.replies, 2);
        assert!(
            !history.is_starved(relay(), server()),
            "accumulating a low share still may not condemn"
        );
        history.record_failure(relay(), server());
        assert!(
            history.is_starved(relay(), server()),
            "the watchdog verdict uses the accumulated record, not one poll"
        );
    }

    #[test]
    fn a_verdict_expires() {
        let now = now_secs();
        let stale = Record {
            forwards: 100,
            replies: 0,
            observed_at: now - VERDICT_TTL.as_secs() - 1,
            failed: true,
        };
        assert!(!stale.is_starved(now), "an expired verdict is no verdict");
        let fresh = Record {
            observed_at: now,
            ..stale
        };
        assert!(fresh.is_starved(now));
    }

    #[test]
    fn the_watchdogs_verdict_is_remembered_even_on_thin_traffic() {
        let mut history = RelayHistory::default();
        // The live case: 21 forwards and 2 replies, under this module's own floor.
        history.record(relay(), server(), 21, 2);
        assert!(!history.is_starved(relay(), server()));
        history.record_failure(relay(), server());
        assert!(
            history.is_starved(relay(), server()),
            "a verdict from the watchdog is evidence in its own right"
        );
    }

    #[test]
    fn a_later_healthy_session_redeems_a_recorded_failure() {
        let mut history = RelayHistory::default();
        history.record_failure(relay(), server());
        assert!(history.is_starved(relay(), server()));
        history.record(relay(), server(), 200, 160);
        assert!(
            !history.is_starved(relay(), server()),
            "the verdict must be redeemable by real traffic, not permanent"
        );
    }

    #[test]
    fn a_24_percent_return_share_carries_the_server() {
        let now = now_secs();
        let mut history = RelayHistory::default();
        // The live healthy window against the old 25% bar: 13/54 = 24.1%.
        history.record(relay(), server(), 54, 13);
        assert!(
            history.carries(relay(), server(), now),
            "one reply in eight is the floor; a quarter is not"
        );
        let mut history = RelayHistory::default();
        // The accumulated live EWR record (93/11 = 11.8%) is below the floor and
        // cannot vouch for the relay, but it still does not condemn it.
        history.record(relay(), server(), 93, 11);
        assert!(!history.carries(relay(), server(), now));
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn fresh_measurement_outranks_a_short_window() {
        let mut history = RelayHistory::default();
        let now = now_secs();
        assert!(
            !history.carries(relay(), server(), now),
            "nothing measured yet"
        );
        history.record(relay(), server(), 200, 160);
        assert!(
            history.carries(relay(), server(), now),
            "80% answered is a relay that carries the server"
        );
        history.record_failure(relay(), server());
        assert!(
            !history.carries(relay(), server(), now),
            "a failure verdict replaces the measurement"
        );
    }

    #[test]
    fn condemned_relays_are_counted_per_server() {
        let mut history = RelayHistory::default();
        let other_relay = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 10), 4434);
        history.record_failure(relay(), server());
        assert_eq!(history.condemned_count(server()), 1);
        history.record_failure(other_relay, server());
        assert_eq!(history.condemned_count(server()), 2);
        assert_eq!(
            history.condemned_count(Ipv4Addr::new(198, 51, 100, 8)),
            0,
            "another server's verdicts are not this server's"
        );
        history.record(other_relay, server(), 200, 180);
        assert_eq!(
            history.condemned_count(server()),
            1,
            "a relay that later carries the server stops counting"
        );
    }

    #[test]
    fn a_relays_record_does_not_leak_to_another_server() {
        let mut history = RelayHistory::default();
        history.record_failure(relay(), server());
        assert!(history.is_starved(relay(), server()));
        assert!(
            !history.is_starved(relay(), Ipv4Addr::new(198, 51, 100, 8)),
            "the same relay may carry one server and not another"
        );
    }

    #[test]
    fn the_memory_survives_a_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("ls-relay-history-{}", std::process::id()));
        let path = dir.join("relay-history.json");
        let mut history = RelayHistory::default();
        history.record_failure(relay(), server());
        history.save(&path).expect("saved");
        let loaded = RelayHistory::load(&path);
        assert!(
            loaded.is_starved(relay(), server()),
            "verdict survived the disk"
        );
        assert!(
            !loaded.carries(relay(), server(), now_secs()),
            "a verdict is not a measurement that carries the server"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pre_verdict_file_loads_as_measurement_only() {
        // The live relay-history.json from the bug: a zero-return record written
        // before the `failed` flag existed. Loading it must not keep excluding a
        // relay on a criterion the fix removed.
        let dir = std::env::temp_dir().join(format!("ls-relay-history-old-{}", std::process::id()));
        let path = dir.join("relay-history.json");
        std::fs::create_dir_all(&dir).expect("dir");
        let document = format!(
            "{{\"entries\":{{\"{}|{}\":{{\"forwards\":179,\"replies\":0,\"observed_at\":{}}}}}}}",
            relay(),
            server(),
            now_secs()
        );
        std::fs::write(&path, document).expect("write");
        let loaded = RelayHistory::load(&path);
        assert!(
            !loaded.is_starved(relay(), server()),
            "a legacy derived verdict is dropped; only an explicit chain may exclude"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!("ls-relay-history-bad-{}", std::process::id()));
        let path = dir.join("relay-history.json");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(&path, "{not json").expect("write");
        let loaded = RelayHistory::load(&path);
        assert!(!loaded.is_starved(relay(), server()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
