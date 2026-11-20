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
//!   reply in eight). It cannot condemn a relay on a low share: game UDP traffic
//!   is asymmetric, and a working relay measured a 24.1% return share on a live
//!   run. A measurement of *no replies at all* over enough traffic is a different
//!   fact, though - that is the starvation signature, and it demotes the relay
//!   without being a condemnable verdict.
//! - **A verdict**, written when the no-reply watchdog confirms the return path
//!   is gone. The finding is written as a starvation signature the moment the
//!   watchdog fires ([`RelayHistory::record_starvation`]); the warn-only mode of
//!   2026-11-20 keeps the session relaying, so no further verdict is produced.
//!   The `failed` flag ([`RelayHistory::record_failure`]) is the retired
//!   exhausted-chain verdict: it is no longer written, but a file written before
//!   that change still loads it and honours it for its six-hour TTL.

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
    /// Set only by [`RelayHistory::record_failure`], the retired exhausted-chain
    /// verdict. The warn-only mode never writes it; a record loaded from a file
    /// written by an older build may still carry it and is honoured until it
    /// expires. A measured ratio never sets it - a low share is evidence about a
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

    /// Whether these counters show the starvation *signature*: enough traffic to
    /// judge and not one reply came back.
    ///
    /// This is deliberately narrower than the carry bar. A low share is normal
    /// for asymmetric game UDP (the live EWR record was 93 forwards for 11
    /// replies, 11.8%, on a relay that was working), so a share below one in
    /// eight is only "unproven". Zero replies is a different fact: the relay
    /// accepted the traffic and nothing came back through it at all. The live
    /// session of 2026-11-19 recorded 4,522 forwards for a G-Core server with
    /// zero replies, `failed` stayed false (the watchdog never exhausted a
    /// chain for that pair), and the chooser kept re-selecting the relay that
    /// had already proven it did not carry that server.
    ///
    /// Time-bounded like the watchdog verdict: a stale record is history, not
    /// evidence about the relay now.
    pub fn starvation_signature(&self, now: u64) -> bool {
        self.replies == 0
            && self.forwards >= MIN_FORWARDS
            && now.saturating_sub(self.observed_at) <= VERDICT_TTL.as_secs()
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
    /// watchdog's window; what is remembered is its verdict, and only a window
    /// that returned nothing at all produces one. Deriving a second verdict from
    /// the raw ratio is how one low-share window excluded a healthy relay for
    /// six hours on a live run. Since the warn-only mode removed the
    /// exhausted-chain verdict this only answers for records carried over from
    /// older files.
    pub fn is_starved(&self, now: u64) -> bool {
        self.failed && now.saturating_sub(self.observed_at) <= VERDICT_TTL.as_secs()
    }
}

/// What a (relay, server) record says about this relay carrying that server.
///
/// The chooser needs one answer, not two booleans: preference ("has this relay
/// proven it carries the server?") and demotion ("has it proven it does not?").
/// Keeping the three cases in one enum stops the two questions from drifting -
/// the live defect was exactly that: the demotion asked only for the watchdog's
/// `failed` flag, so a record with 4,522 forwards and zero replies was neither
/// condemned nor distrusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryVerdict {
    /// A fresh measurement at or above the carry bar: the relay demonstrably
    /// carried this server's traffic recently.
    Proven,
    /// The watchdog's verdict, or a fresh record whose counters show the
    /// starvation signature. The relay is excluded for this server.
    Starved,
    /// Nothing known, or a measurement that has not reached the carry bar. It
    /// may still be the right relay; no evidence either way.
    Unproven,
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

    /// Record the retired exhausted-chain verdict for the pair.
    ///
    /// This was the only way a relay was remembered as not carrying a server:
    /// the caller wrote it once the watchdog had spent its re-tests and every
    /// window of the chain saw `from_relay == 0` and `injected == 0`. The
    /// warn-only mode of 2026-11-20 removed that path - there is no chain and no
    /// retry budget any more - so nothing in the GUI calls this. It is kept so
    /// the persisted `failed` verdict still has a constructor for the loader's
    /// format and the tests that pin how such a record is read; a record loaded
    /// from an older file keeps its meaning until it expires.
    #[allow(dead_code)]
    pub fn record_failure(&mut self, relay: SocketAddrV4, server: Ipv4Addr) {
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = entry.forwards.max(MIN_FORWARDS);
        entry.replies = 0;
        entry.observed_at = now_secs();
        entry.failed = true;
        self.trim();
    }

    /// Record the watchdog's starvation finding for the pair.
    ///
    /// Called the moment the watchdog confirms `NoReplyFromRelay` for the pair,
    /// so the very next ranking already excludes the relay instead of
    /// re-selecting the path that is starving the session. `failed` is
    /// deliberately left false: a finding from one warning episode is not the
    /// retired exhausted-chain verdict, and a later session that really carries
    /// the server is free to redeem it. The record takes the starved shape
    /// anyway (enough forwards, no replies), which is the signature
    /// [`Record::starvation_signature`] reads.
    ///
    /// The counters restart from [`MIN_FORWARDS`] rather than keeping the
    /// accumulated ones: a measurement added later must be able to lift the
    /// share back above the carry bar, and `replies` cannot climb out from under
    /// a huge `forwards` base within one match.
    pub fn record_starvation(&mut self, relay: SocketAddrV4, server: Ipv4Addr) {
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = MIN_FORWARDS;
        entry.replies = 0;
        entry.observed_at = now_secs();
        entry.failed = false;
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
    /// The chooser uses this to stop trying: with eight candidates, a server no
    /// relay carries would otherwise keep choosing paths already measured as
    /// starving it, and every attempt is another match spent proving what the
    /// earlier ones showed. The count is the same evidence the ranking demotes
    /// on - the starvation signature, or a legacy `failed` verdict loaded from a
    /// file written before the warn-only mode - because both come from the same
    /// counters. A relay that later carries the server stops counting.
    pub fn condemned_count(&self, server: Ipv4Addr) -> usize {
        let suffix = format!("|{server}");
        let now = now_secs();
        self.entries
            .iter()
            .filter(|(key, record)| {
                key.ends_with(&suffix)
                    && (record.is_starved(now) || record.starvation_signature(now))
            })
            .count()
    }

    /// Whether this relay is on record as not carrying this server.
    pub fn is_starved(&self, relay: SocketAddrV4, server: Ipv4Addr) -> bool {
        self.entries
            .get(&key(relay, server))
            .is_some_and(|record| record.is_starved(now_secs()))
    }

    /// What this pair's record says about the relay carrying the server.
    ///
    /// One entry point for both questions the chooser asks, so "has it proven
    /// it carries this server" and "has it proven it does not" can never be read
    /// from different rules of the same record. Starvation is checked first and
    /// covers both a legacy `failed` verdict loaded from an older file and the
    /// starvation signature a live record can show; a record that is neither
    /// condemned nor above the carry bar is [`CarryVerdict::Unproven`], which is
    /// not a demotion - a relay with no history is a fair candidate.
    pub fn carry_verdict(&self, relay: SocketAddrV4, server: Ipv4Addr) -> CarryVerdict {
        let Some(record) = self.entries.get(&key(relay, server)) else {
            return CarryVerdict::Unproven;
        };
        let now = now_secs();
        // The legacy-verdict rule is read through its own method so the two
        // callers of `failed` can never drift: `is_starved` stays exactly the
        // persisted verdict it has always been.
        if self.is_starved(relay, server) || record.starvation_signature(now) {
            return CarryVerdict::Starved;
        }
        if self.carries(relay, server, now) {
            return CarryVerdict::Proven;
        }
        CarryVerdict::Unproven
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
        // The warn-only mode no longer writes `failed`; its live finding is the
        // starvation signature, and the chooser's cap must count that evidence
        // too or the cap would stop firing once the legacy verdicts expired.
        let signature_relay = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 11), 4434);
        history.record(signature_relay, server(), 200, 0);
        assert_eq!(
            history.condemned_count(server()),
            2,
            "a starvation signature is a condemnation for the cap"
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

    // ── The starvation signature (carry verdict) ─────────────────────────────

    #[test]
    fn a_starved_record_is_demoted_even_without_a_failed_verdict() {
        // The live G-Core case: traffic went through a relay and not one reply
        // came back, but the watchdog never exhausted a chain, so `failed` stayed
        // false and `is_starved` - by design, it only reads the explicit verdict -
        // did not demote the relay. The chooser kept re-selecting it.
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 4_522, 0);
        assert!(
            !history.is_starved(relay(), server()),
            "keep the watchdog-verdict semantics: a signature is not a `failed` record"
        );
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Starved,
            "a record with enough forwards and no reply is a starvation signature"
        );
    }

    #[test]
    fn the_watchdogs_starvation_finding_is_recorded_without_a_failure() {
        let mut history = RelayHistory::default();
        history.record_starvation(relay(), server());
        assert!(
            !history.is_starved(relay(), server()),
            "the first finding is not the exhausted-chain verdict"
        );
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Starved
        );
        let record = history.get(relay(), server()).expect("recorded");
        assert!(!record.failed, "a finding must not masquerade as a verdict");
        assert_eq!(record.forwards, MIN_FORWARDS);
        assert_eq!(record.replies, 0);
    }

    #[test]
    fn a_healthy_relay_is_not_demoted() {
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 200, 160);
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Proven,
            "80% answered is a relay that carries the server"
        );
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn an_unknown_record_is_not_demoted() {
        let history = RelayHistory::default();
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Unproven,
            "no evidence is not evidence against the relay"
        );
        assert!(!history.is_starved(relay(), server()));
    }

    #[test]
    fn a_low_measured_share_is_not_a_starvation_signature() {
        // The live EWR record the docs protect: 93 forwards, 11 replies (11.8%)
        // on a relay that was working. Below the carry bar, so not proven - but
        // it returned traffic, so it is not starved either.
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 93, 11);
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Unproven,
            "a low share is normal game UDP, not starvation"
        );
    }

    #[test]
    fn a_thin_zero_reply_session_is_not_a_starvation_signature() {
        let mut history = RelayHistory::default();
        // Ten forwards with nothing back is a quiet match, not a broken relay.
        history.record(relay(), server(), MIN_FORWARDS - 1, 0);
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Unproven
        );
    }

    #[test]
    fn a_stale_starvation_signature_expires() {
        let now = now_secs();
        let record = Record {
            forwards: 500,
            replies: 0,
            observed_at: now - VERDICT_TTL.as_secs() - 1,
            failed: false,
        };
        assert!(
            !record.starvation_signature(now),
            "a six-hour-old zero-reply record is history, not evidence about now"
        );
    }

    #[test]
    fn a_recorded_starvation_is_redeemed_by_a_real_carry_measurement() {
        let mut history = RelayHistory::default();
        history.record_starvation(relay(), server());
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Starved
        );
        // A later session that really carries the server must be able to lift the
        // record back to proven; the starved counters restart small so that is
        // reachable within one match.
        history.record(relay(), server(), 200, 160);
        assert!(
            !history.is_starved(relay(), server()),
            "a finding is redeemable, not a permanent exclusion"
        );
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Proven,
            "the later measurement proves the relay carries this server"
        );
    }

    #[test]
    fn a_watchdog_verdict_is_starved_whatever_its_counters() {
        let mut history = RelayHistory::default();
        history.record(relay(), server(), 21, 2);
        history.record_failure(relay(), server());
        assert_eq!(
            history.carry_verdict(relay(), server()),
            CarryVerdict::Starved,
            "the explicit verdict outranks the counters"
        );
    }
}
