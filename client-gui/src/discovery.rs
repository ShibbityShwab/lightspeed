//! GUI discovery worker plumbing — the discovery and measurement rules live in
//! the shared crate.
//!
//! MOVED at WF-081 W2-1 stage S2 (`docs/design/gui-core-boundary.md` §3.2.1/§6):
//! registry lookup, `/health` probing, the relay race and the five-probe client
//! leg measurement now live in `lightspeed_client::app::discovery` so the CLI can
//! adopt the same answers at S6 and the GUI cannot drift a second copy of the
//! rules. What stays here is the one-shot `mpsc` thread plumbing the boundary
//! explicitly keeps in a front end: a worker thread runs the blocking function
//! and the egui `update()` loop only ever polls the receiver.

use std::net::SocketAddrV4;
use std::sync::mpsc::{self, Receiver};

pub use lightspeed_client::app::discovery::{
    friendly_label, health_endpoint, probe_blocking, DiscoveryOutcome, RelayHealth, RelayInfo,
    RelaySample,
};

/// Start a discovery attempt on a background thread; the receiver yields
/// exactly one outcome.
pub fn spawn_discovery() -> Receiver<DiscoveryOutcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(lightspeed_client::app::discovery::discover_blocking());
    });
    rx
}

/// Start a `/health` probe for `addr` on a background thread.
pub fn spawn_health_probe(addr: SocketAddrV4) -> Receiver<Result<RelayHealth, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(lightspeed_client::app::discovery::probe_blocking(addr));
    });
    rx
}

/// Race every address in `addrs` for the fastest one on a background thread.
pub fn spawn_relay_race(addrs: Vec<SocketAddrV4>) -> Receiver<Option<SocketAddrV4>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(lightspeed_client::app::discovery::race_blocking(&addrs));
    });
    rx
}

/// Measure every address for the whole-path ranking on a background thread; the
/// receiver yields the full sample set once.
pub fn spawn_relay_measurement(
    addrs: Vec<SocketAddrV4>,
) -> Receiver<Vec<(SocketAddrV4, Option<RelaySample>)>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(lightspeed_client::app::discovery::measure_spreads_blocking(
            &addrs,
        ));
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::spawn_relay_race;
    use std::time::Duration;

    /// The wrapper contract the GUI actually depends on: the worker always
    /// answers, even for an empty race, so the poll never blocks forever.
    #[test]
    fn spawn_relay_race_delivers_a_result() {
        let rx = spawn_relay_race(Vec::new());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).ok(),
            Some(None),
            "an empty race resolves to no winner"
        );
    }
}
