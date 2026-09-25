//! `--diagnose` mode: answer the only question that matters, "does LightSpeed
//! help me?", for one game server.
//!
//! It measures the direct (ICMP) path and the relayed (tunnelled) path over a
//! short bounded window using the existing [`crate::latency`] tracker, then
//! prints a plain verdict: both medians, their difference, and a one-line
//! conclusion. When either path produced too few samples it says exactly that
//! instead of guessing. Nothing here reports telemetry; the verdict is local.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lightspeed_protocol::TunnelHeader;

use crate::latency::{self, LatencyTracker};

/// Minimum replies on each path before its median is trusted.
pub const MIN_SAMPLES: usize = 3;
/// Default upper bound on the whole relayed sampling window.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5);
/// Gap between relayed probes during the window.
pub const PROBE_INTERVAL: Duration = Duration::from_millis(500);
/// Differences at or below this many ms are treated as noise, not a saving or a
/// penalty.
pub const NOISE_FLOOR_MS: f32 = 1.0;
/// Upper bound on the direct ICMP burst, so an unreachable server cannot stall
/// the whole mode.
pub const DIRECT_BURST_BUDGET: Duration = Duration::from_secs(3);
/// Per-probe wait for the relayed reply.
pub const RELAYED_PROBE_TIMEOUT: Duration = Duration::from_millis(600);

/// One bounded diagnostic sample: the two medians under test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Median direct ICMP RTT in ms, or `None` when too few replies arrived.
    pub direct_ms: Option<f32>,
    /// Median tunnelled RTT in ms, or `None` when too few replies arrived.
    pub relayed_ms: Option<f32>,
}

/// Why a diagnosis could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insufficient {
    /// The direct path produced too few replies.
    Direct,
    /// The relayed path produced too few replies.
    Relayed,
    /// Neither path produced enough replies.
    Both,
}

/// The plain conclusion drawn from a [`Sample`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    /// The relayed path is faster by `saved_ms`.
    Saved { saved_ms: f32 },
    /// The direct path is faster by `penalty_ms`.
    Worse { penalty_ms: f32 },
    /// The two paths are within [`NOISE_FLOOR_MS`] of each other.
    Neutral { difference_ms: f32 },
    /// Not enough samples to judge.
    Insufficient(Insufficient),
}

/// Source of one bounded diagnostic sample. The real implementation drives the
/// existing latency tracker; tests substitute a scripted source.
pub trait DiagnosticSource: Send {
    /// Measure `server` over one bounded window.
    fn sample(&mut self, server: SocketAddrV4) -> Sample;
}

/// Measure `server` through `source` and turn it into a verdict.
pub fn diagnose(source: &mut dyn DiagnosticSource, server: SocketAddrV4) -> (Sample, Verdict) {
    let sample = source.sample(server);
    let verdict = verdict(sample.direct_ms, sample.relayed_ms);
    (sample, verdict)
}

/// Turn the two measured medians into a plain verdict.
pub fn verdict(direct_ms: Option<f32>, relayed_ms: Option<f32>) -> Verdict {
    match (direct_ms, relayed_ms) {
        (Some(direct), Some(relayed)) => {
            let difference = direct - relayed;
            if difference >= NOISE_FLOOR_MS {
                Verdict::Saved {
                    saved_ms: difference,
                }
            } else if difference <= -NOISE_FLOOR_MS {
                Verdict::Worse {
                    penalty_ms: -difference,
                }
            } else {
                Verdict::Neutral {
                    difference_ms: difference,
                }
            }
        }
        (None, None) => Verdict::Insufficient(Insufficient::Both),
        (None, Some(_)) => Verdict::Insufficient(Insufficient::Direct),
        (Some(_), None) => Verdict::Insufficient(Insufficient::Relayed),
    }
}

/// A median is only usable when it came from at least [`MIN_SAMPLES`] replies.
pub fn trusted(median_ms: Option<f32>, samples: usize) -> Option<f32> {
    if samples >= MIN_SAMPLES {
        median_ms
    } else {
        None
    }
}

fn fmt_ms(value: Option<f32>) -> String {
    match value {
        Some(ms) => format!("{ms:.1} ms"),
        None => "not enough samples".to_string(),
    }
}

fn insufficient_line(kind: Insufficient) -> &'static str {
    match kind {
        Insufficient::Direct => {
            "Not enough direct replies to judge. ICMP echo needs no root on Linux \
             and macOS; on Windows run an elevated session and try again."
        }
        Insufficient::Relayed => {
            "Not enough relayed replies. The game server did not answer a probe \
             through the relay, which is normal unless it answers probe traffic. \
             Point --target at an echo-capable host, or diagnose while connected."
        }
        Insufficient::Both => {
            "Not enough replies on either path to judge. Check the relay address \
             and that the target answers probes, then try again."
        }
    }
}

/// Render the human report for `server`.
pub fn render(server: SocketAddrV4, sample: Sample, verdict: Verdict) -> String {
    let mut out = String::new();
    out.push_str(&format!("LightSpeed diagnosis for {server}\n"));
    out.push_str(&format!(
        "  Direct (ICMP) RTT:     {}\n",
        fmt_ms(sample.direct_ms)
    ));
    out.push_str(&format!(
        "  Relayed (tunnel) RTT:  {}\n",
        fmt_ms(sample.relayed_ms)
    ));
    match verdict {
        Verdict::Saved { saved_ms } => {
            out.push_str(&format!(
                "  Difference:            {saved_ms:.0} ms faster through the relay\n"
            ));
            out.push_str(&format!(
                "  LightSpeed is saving you about {saved_ms:.0} ms here.\n"
            ));
        }
        Verdict::Worse { penalty_ms } => {
            out.push_str(&format!(
                "  Difference:            {penalty_ms:.0} ms slower through the relay\n"
            ));
            out.push_str("  The relay is not helping on this path; connect directly.\n");
        }
        Verdict::Neutral { difference_ms } => {
            out.push_str(&format!(
                "  Difference:            {difference_ms:.1} ms (within noise)\n"
            ));
            out.push_str("  The relay is not helping on this path; connect directly.\n");
        }
        Verdict::Insufficient(kind) => {
            out.push_str("  Difference:            not enough samples\n");
            out.push_str(&format!("  {}\n", insufficient_line(kind)));
        }
    }
    out.push_str("  Note: the direct figure is ICMP, the relayed figure is a tunnelled probe.\n");
    out.push_str("        Different instruments, so read the difference as an estimate.\n");
    out
}

/// Real [`DiagnosticSource`]: drives the process latency tracker with one direct
/// ICMP burst and a bounded run of tunnelled probes through `relay`.
struct LiveDiagnostics {
    tracker: Arc<LatencyTracker>,
    relay: SocketAddrV4,
    window: Duration,
    socket: std::net::UdpSocket,
    seq: u16,
}

impl LiveDiagnostics {
    fn new(
        tracker: Arc<LatencyTracker>,
        relay: SocketAddrV4,
        window: Duration,
    ) -> io::Result<Self> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(RELAYED_PROBE_TIMEOUT))?;
        Ok(Self {
            tracker,
            relay,
            window,
            socket,
            seq: 0,
        })
    }

    fn measure_relayed(&mut self, server: SocketAddrV4) -> Option<f32> {
        let deadline = Instant::now() + self.window;
        let local = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
        while Instant::now() < deadline {
            self.seq = self.seq.wrapping_add(1);
            let seq = self.seq;
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u32;
            let payload = format!("LS_DIAG_{seq}");
            let header = TunnelHeader::new(seq, ts, local, server)
                .with_session_token(crate::session::session_token());
            let packet = header.encode_with_payload(payload.as_bytes());

            self.tracker.note_outbound(*server.ip());
            if self.socket.send_to(&packet, self.relay).is_err() {
                std::thread::sleep(PROBE_INTERVAL);
                continue;
            }

            let mut buf = [0u8; 2048];
            let sent = Instant::now();
            while sent.elapsed() < RELAYED_PROBE_TIMEOUT {
                match self.socket.recv_from(&mut buf) {
                    Ok((len, _)) => {
                        if let Ok((_header, reply)) = TunnelHeader::decode_with_payload(&buf[..len])
                        {
                            if reply == payload.as_bytes() {
                                self.tracker.record_inbound(*server.ip());
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            std::thread::sleep(PROBE_INTERVAL);
        }
        trusted(
            self.tracker.relayed_p50_ms(),
            self.tracker.relayed_samples() as usize,
        )
    }
}

impl DiagnosticSource for LiveDiagnostics {
    fn sample(&mut self, server: SocketAddrV4) -> Sample {
        // Direct runs on its own thread while the relayed window is sampled, so
        // the whole mode is bounded by the window, not the sum of both paths.
        let tracker = Arc::clone(&self.tracker);
        let ip = *server.ip();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(tracker.run_direct_burst(ip));
        });
        let relayed_ms = self.measure_relayed(server);
        let direct_ms = rx.recv_timeout(DIRECT_BURST_BUDGET).ok().flatten();
        Sample {
            direct_ms,
            relayed_ms,
        }
    }
}

/// Run the on-demand diagnosis for `server` through `relay`.
pub async fn run_diagnose(
    server: SocketAddrV4,
    relay: SocketAddrV4,
    window: Duration,
) -> anyhow::Result<()> {
    latency::install_measurement();
    let tracker = latency::global()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("latency tracker unavailable"))?;
    let (sample, verdict) = tokio::task::spawn_blocking(move || {
        let mut source = LiveDiagnostics::new(tracker, relay, window)?;
        Ok::<_, io::Error>(diagnose(&mut source, server))
    })
    .await??;
    print!("{}", render(server, sample, verdict));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        sample: Sample,
    }

    impl DiagnosticSource for FakeSource {
        fn sample(&mut self, _server: SocketAddrV4) -> Sample {
            self.sample
        }
    }

    fn server() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 28015)
    }

    fn diagnose_fake(direct_ms: Option<f32>, relayed_ms: Option<f32>) -> Verdict {
        let mut source = FakeSource {
            sample: Sample {
                direct_ms,
                relayed_ms,
            },
        };
        diagnose(&mut source, server()).1
    }

    #[test]
    fn verdict_reports_saved_when_the_relay_is_faster() {
        assert_eq!(
            diagnose_fake(Some(50.0), Some(20.0)),
            Verdict::Saved { saved_ms: 30.0 }
        );
    }

    #[test]
    fn verdict_reports_worse_when_the_relay_is_slower() {
        assert_eq!(
            diagnose_fake(Some(20.0), Some(45.0)),
            Verdict::Worse { penalty_ms: 25.0 }
        );
    }

    #[test]
    fn verdict_reports_neutral_when_the_paths_are_within_noise() {
        assert_eq!(
            diagnose_fake(Some(20.0), Some(20.5)),
            Verdict::Neutral {
                difference_ms: -0.5
            }
        );
    }

    #[test]
    fn verdict_reports_insufficient_when_the_direct_path_has_no_samples() {
        assert_eq!(
            diagnose_fake(None, Some(20.0)),
            Verdict::Insufficient(Insufficient::Direct)
        );
    }

    #[test]
    fn verdict_reports_insufficient_when_the_relayed_path_has_no_samples() {
        assert_eq!(
            diagnose_fake(Some(20.0), None),
            Verdict::Insufficient(Insufficient::Relayed)
        );
    }

    #[test]
    fn verdict_reports_insufficient_when_neither_path_has_samples() {
        assert_eq!(
            diagnose_fake(None, None),
            Verdict::Insufficient(Insufficient::Both)
        );
    }

    #[test]
    fn trusted_requires_the_minimum_sample_count() {
        assert_eq!(trusted(Some(20.0), MIN_SAMPLES), Some(20.0));
        assert_eq!(trusted(Some(20.0), MIN_SAMPLES - 1), None);
        assert_eq!(trusted(None, MIN_SAMPLES), None);
    }

    #[test]
    fn render_states_the_numbers_and_the_plain_saving() {
        let sample = Sample {
            direct_ms: Some(50.0),
            relayed_ms: Some(20.0),
        };
        let text = render(server(), sample, Verdict::Saved { saved_ms: 30.0 });
        assert!(text.contains("203.0.113.7:28015"));
        assert!(text.contains("50.0 ms"), "direct RTT must be shown");
        assert!(text.contains("20.0 ms"), "relayed RTT must be shown");
        assert!(
            text.contains("saving you about 30 ms"),
            "a saving must be stated plainly: {text}"
        );
    }

    #[test]
    fn render_never_overclaims_when_the_relay_is_slower() {
        let sample = Sample {
            direct_ms: Some(20.0),
            relayed_ms: Some(45.0),
        };
        let text = render(server(), sample, Verdict::Worse { penalty_ms: 25.0 });
        assert!(
            text.contains("not helping"),
            "a relay that is slower must be called out: {text}"
        );
        assert!(
            text.contains("connect directly"),
            "the user must be told what to do: {text}"
        );
    }

    #[test]
    fn render_says_not_enough_samples_instead_of_guessing() {
        let sample = Sample {
            direct_ms: None,
            relayed_ms: None,
        };
        let text = render(server(), sample, Verdict::Insufficient(Insufficient::Both));
        assert!(
            text.contains("not enough samples"),
            "insufficient data must be stated, never guessed: {text}"
        );
    }
}
