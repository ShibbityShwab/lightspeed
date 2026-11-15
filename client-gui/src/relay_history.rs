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

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Share of replies to forwards below which a relay is called starved. The
/// no-reply watchdog uses the same quarter: a relay returning fewer than a
/// quarter of the traffic it carried is not carrying it at all.
pub const STARVED_SHARE: f64 = 0.25;

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

/// One (relay, server) record: the counters as last observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// Packets this client pushed through the relay for the server.
    pub forwards: u64,
    /// Replies that came back through the same relay for the same server.
    pub replies: u64,
    /// Unix seconds when the counters were last observed.
    pub observed_at: u64,
}

impl Record {
    pub fn answered_share(&self) -> f64 {
        if self.forwards == 0 {
            return 0.0;
        }
        (self.replies as f64 / self.forwards as f64).clamp(0.0, 1.0)
    }

    /// Whether this record is damning: enough traffic to judge, and too few
    /// replies for the relay to be considered to be carrying the server.
    ///
    /// Thin evidence has to clear a stricter bar. A record at the floor with a
    /// share a hair under a quarter is a quiet stretch, not a verdict: live traffic
    /// produced 41 forwards and 10 replies (24.4%) and condemned a relay on it.
    /// Below twice the floor the share must be half as bad, which is the difference
    /// between "this relay is dropping the server" and "this server was quiet".
    pub fn is_starved(&self, now: u64) -> bool {
        let share_bar = if self.forwards < MIN_FORWARDS * 2 {
            STARVED_SHARE / 2.0
        } else {
            STARVED_SHARE
        };
        self.forwards >= MIN_FORWARDS
            && self.answered_share() < share_bar
            && now.saturating_sub(self.observed_at) <= VERDICT_TTL.as_secs()
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
    pub fn record(&mut self, relay: SocketAddrV4, server: Ipv4Addr, forwards: u64, replies: u64) {
        if forwards == 0 && replies == 0 {
            return;
        }
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = entry.forwards.saturating_add(forwards);
        entry.replies = entry.replies.saturating_add(replies);
        entry.observed_at = now_secs();
        self.trim();
    }

    /// Record the watchdog's verdict for the pair.
    ///
    /// The no-reply watchdog is the validated instrument: it fires only after its
    /// own settle window and floor, and the GUI records this once both re-tests
    /// have failed for the same server. Duplicating its threshold here meant a
    /// live case it condemned (21 forwards, 2 replies, 10%) fell below this
    /// module's own floor and was not remembered at all.
    pub fn record_failure(&mut self, relay: SocketAddrV4, server: Ipv4Addr) {
        let entry = self.entries.entry(key(relay, server)).or_default();
        entry.forwards = entry.forwards.max(MIN_FORWARDS);
        entry.replies = 0;
        entry.observed_at = now_secs();
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
                record.forwards >= MIN_FORWARDS
                    && record.answered_share() >= STARVED_SHARE
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
    /// must not stop the client from starting.
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
    fn a_relay_returning_few_replies_is_marked_starved() {
        let mut history = RelayHistory::default();
        // 100 forwarded, 2 answered: the LAX case, live.
        history.record(relay(), server(), 100, 2);
        assert!(history.is_starved(relay(), server()));
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
            history.is_starved(relay(), server()),
            "the verdict needs the accumulated total, not one poll"
        );
    }

    #[test]
    fn a_verdict_expires() {
        let now = now_secs();
        let stale = Record {
            forwards: 100,
            replies: 1,
            observed_at: now - VERDICT_TTL.as_secs() - 1,
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
        assert!(
            !history.is_starved(relay(), server()),
            "thin by volume alone"
        );
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
    fn thin_evidence_needs_a_wider_margin() {
        let now = now_secs();
        let marginal = Record {
            forwards: MIN_FORWARDS + 1,
            replies: (MIN_FORWARDS + 1) / 5,
            observed_at: now,
        };
        assert!(
            !marginal.is_starved(now),
            "24% on thin traffic is a quiet stretch, not a verdict"
        );
        let damning = Record {
            forwards: MIN_FORWARDS + 1,
            replies: (MIN_FORWARDS + 1) / 20,
            observed_at: now,
        };
        assert!(damning.is_starved(now));
        let plenty = Record {
            forwards: MIN_FORWARDS * 10,
            replies: MIN_FORWARDS * 2,
            observed_at: now,
        };
        assert!(
            plenty.is_starved(now),
            "20% over a large sample is still a verdict"
        );
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
        history.record(relay(), server(), 100, 1);
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
        history.record(relay(), server(), 100, 3);
        history.save(&path).expect("saved");
        let loaded = RelayHistory::load(&path);
        assert!(loaded.is_starved(relay(), server()), "verdict survived");
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
