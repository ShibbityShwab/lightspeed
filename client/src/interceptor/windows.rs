//! Windows `WinDivertInterceptor` - kernel-level traffic interception via WinDivert.
//!
//! This is the gold-standard implementation on Windows.  WinDivert is a signed
//! NDIS lightweight filter driver that holds each matching packet in the
//! kernel until userspace either re-injects or drops it - identical in
//! principle to how other packet-capture tools operate.
//!
//! ## Improvements over legacy `windivert_redirect.rs`
//!
//! - Implements the OOP [`TrafficInterceptor`] trait.
//! - ONE static network-layer divert handle is opened at start and kept for the
//!   life of the interception: no per-flow handle rebuilds, no swap on flow
//!   change, no close/reopen on flow events. The filter text is built once by
//!   [`build_capture_filter`] from the config's [`CapturePolicy`].
//! - The network layer cannot match `processId` (only the Flow/Socket layers
//!   can), so the filter never tries: process-targeted capture takes the whole
//!   process's outbound UDP and attributes each packet in userspace.
//! - Routes pre-seeded from `ProcessScanner` let the engine skip the debounce
//!   accumulation window: interception starts with the first packet.
//! - Stale-server timeout unchanged (5 s) - still auto-resets on map change.
//!
//! ## The data plane
//!
//! - **Process mode** ([`CapturePolicy::ProcessAll`], a picked process): every
//!   captured packet's flow is looked up in the bounded
//!   [`FlowMap`](super::flows::FlowMap) (`super::flows`, seeded from the
//!   target's live routes). A flow attributed to the process is relayed - the
//!   original is dropped and the payload is forwarded through the tunnel - and
//!   every other packet (unattributed, over the tunnel cap, or excluded) is
//!   re-injected immediately on the direct path. Nothing is silently dropped
//!   and no unattributed flow is relayed.
//! - **Port-scoped mode** ([`CapturePolicy::PortRange`], the built-in game
//!   path): the same static handle and the same userspace single-server
//!   `ServerTracker`, unchanged.
//!
//! The cap is enforced where the map is used: at most `MAX_TUNNELED_FLOWS`
//! flows tunnel, a warning is raised from `WARN_TUNNELED_FLOWS`, and a flow
//! over the cap goes direct and is counted UNATTRIBUTED (`super::flows`).
//!
//! ## Exclusions
//!
//! A packet to an excluded destination never enters the tunnel. The filter text
//! excludes the relay's own address(es) and the [infra denylist
//! ports][deny]; the userspace gate [`destination_is_excluded`] repeats those
//! rules and adds the non-public-destination rule through the existing
//! [`is_public_ipv4`](super::process_scanner::is_public_ipv4), which the filter
//! language cannot express.
//!
//! [deny]: INFRA_DENYLIST_PORTS
//!
//! ## Reused from the proven engine, not re-invented
//!
//! The relay loop is the one proven in `capture/windivert_redirect.rs`:
//! `parse_ipv4_udp` and `build_ipv4_udp` are imported from it verbatim, and the
//! intercept -> drop-original -> wrap in a `TunnelHeader` -> send to the relay
//! -> unwrap the reply -> inject a spoofed server packet shape mirrors that
//! engine's UDP path. What this backend adds is the userspace attribution step
//! and the exclusions.
//!
//! ## Requirements
//! - Windows only.
//! - `windivert-redirect` Cargo feature must be enabled.
//! - `WinDivert64.sys` + `WinDivert.dll` must sit next to the exe.
//! - Process must run as **Administrator**, and process-targeted interception
//!   needs a real process to point at: live behaviour is an F5 field check, and
//!   the userspace logic below is what CI can cover.

// Imports needed by both the full WinDivert impl and the stub:
use super::traits::{InterceptorConfig, InterceptorHandle, TrafficInterceptor};

// The capture policy, the exclusions and the per-packet routing decision are
// plain userspace logic with no WinDivert dependency: they compile into this
// module on every Windows build and are unit-tested without the driver.
use super::flows::{DirectReason, FlowKey, FlowMap, FlowVerdict};
use super::socket_owners::OwnerCache;
use super::CapturePolicy;
use crate::games::TransportProto;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Instant;

// Imports only needed when WinDivert is actually compiled in:
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::order::{effective_seeds, Decision, ServerTracker, TrackerConfig};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::recovery::RecvBackoff;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::teardown::{recv_should_break, TeardownAck};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::traits::{InterceptorCounters, PlatformTeardown, Route};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::windivert_handle::{is_fwp_in_use, OwnedHandle};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use crate::capture::windivert_redirect::{build_ipv4_udp, parse_ipv4_udp};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use crate::tunnel::adaptive::InlineFecGate;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::sync::Arc;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::time::Duration;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use windivert_sys::address::WINDIVERT_ADDRESS;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use windivert_sys::{WinDivertFlags, WinDivertLayer, WinDivertShutdownMode};

// ─────────────────────────────────────────────────────────────────────────────
//  Struct
// ─────────────────────────────────────────────────────────────────────────────

/// Windows traffic interceptor backed by WinDivert.
///
/// Wraps the kernel-driver intercept+inject pattern in a clean OOP shell.
/// Obtain one through the module-level factory `create_interceptor()`.
pub struct WinDivertInterceptor;

impl WinDivertInterceptor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WinDivertInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Capture policy, exclusions and per-packet routing (feature-independent)
// ─────────────────────────────────────────────────────────────────────────────

/// The infrastructure services a relayed session must never send through the
/// tunnel.
///
/// DNS, DHCP, mDNS, SSDP and NetBIOS are infrastructure chatter: relaying them
/// through a game relay is wrong even for "all traffic of this process". The
/// filter text excludes these destination ports and [`destination_is_excluded`]
/// repeats the rule in userspace, so a packet to one of them goes direct and
/// never enters the tunnel.
///
/// This is the client-side mirror of section 5.5 of
/// `docs/design/custom-games-and-process-targeting.md`: the filter builder and
/// the userspace gate both read this one list.
pub const INFRA_DENYLIST_PORTS: [u16; 8] = [53, 67, 68, 5353, 1900, 137, 138, 139];

/// Whether `dst` is excluded from the tunnel.
///
/// Three rules, every one of them "never relay this":
/// 1. the destination port is in [`INFRA_DENYLIST_PORTS`];
/// 2. the destination is one of the relay's own addresses - the client's own
///    tunnel traffic must never be re-captured and relayed back into itself;
/// 3. the destination is not a public IPv4 address
///    ([`is_public_ipv4`](super::process_scanner::is_public_ipv4)), so a local
///    or special-use destination is answered on the machine and has no business
///    in a relay.
///
/// The filter text can express (1) and (2); (3) is a userspace rule because the
/// WinDivert filter language has no notion of routability. This gate is what
/// makes "a packet to an excluded destination never enters the tunnel" true for
/// the non-public case as well.
pub fn destination_is_excluded(dst: SocketAddrV4, relay_addrs: &[Ipv4Addr]) -> bool {
    INFRA_DENYLIST_PORTS.contains(&dst.port())
        || relay_addrs.contains(dst.ip())
        || !super::process_scanner::is_public_ipv4(*dst.ip())
}

/// What the data plane should do with one captured outbound packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundRoute {
    /// Relay it: the original is dropped and the payload is forwarded through
    /// the tunnel (the injected spoofed reply closes the loop).
    Tunnel,
    /// Re-inject the original packet unchanged on the direct path, because the
    /// destination is excluded.
    Excluded,
    /// Re-inject the original unchanged: the flow is not attributed to the
    /// selected process, or it is over the tunnel cap, which the map reports as
    /// [`DirectReason::Unattributed`].
    Reinject(DirectReason),
}

/// The userspace attribution decision for one captured packet.
///
/// Every captured packet passes through here, so this is the one place the
/// process-targeted path decides relay-versus-direct. The exclusion gate runs
/// first - an excluded destination never reaches the map, so it can never take
/// a tunnel slot - then the packet's flow is looked up in `map`.
///
/// Fail-open is the map's invariant, restated at the call site: the only
/// outcomes are [`OutboundRoute::Tunnel`], [`OutboundRoute::Excluded`] and a
/// re-injection. There is no drop path.
pub fn route_outbound(
    map: &mut FlowMap,
    src: SocketAddrV4,
    dst: SocketAddrV4,
    relay_addrs: &[Ipv4Addr],
    now: Instant,
    payload_len: usize,
) -> OutboundRoute {
    if destination_is_excluded(dst, relay_addrs) {
        return OutboundRoute::Excluded;
    }
    match map.observe(
        FlowKey::new(src, dst, TransportProto::Udp),
        now,
        payload_len,
    ) {
        FlowVerdict::Tunnel(_) => OutboundRoute::Tunnel,
        FlowVerdict::Direct(reason) => OutboundRoute::Reinject(reason),
    }
}

/// Build the one static network-layer filter this backend uses.
///
/// The handle is opened once from this string and never rebuilt, so the filter
/// must be a superset of everything that may ever be tunneled: for
/// [`CapturePolicy::ProcessAll`] that is every outbound UDP datagram, and for
/// [`CapturePolicy::PortRange`] the configured inclusive range. `processId`
/// appears nowhere, because the network layer cannot match it.
///
/// The exclusions are folded in here - every [`INFRA_DENYLIST_PORTS`] port and
/// every relay address - so the kernel never even hands an excluded packet to
/// the data plane. [`destination_is_excluded`] repeats the rules in userspace
/// for the non-public case the filter language cannot express.
pub fn build_capture_filter(policy: CapturePolicy, relay_addrs: &[Ipv4Addr]) -> String {
    let mut filter = String::from("udp and outbound");
    if let CapturePolicy::PortRange { lo, hi } = policy {
        if lo == hi {
            filter.push_str(&format!(" and udp.DstPort == {lo}"));
        } else {
            filter.push_str(&format!(" and udp.DstPort >= {lo} and udp.DstPort <= {hi}"));
        }
    }
    for port in INFRA_DENYLIST_PORTS {
        filter.push_str(&format!(" and udp.DstPort != {port}"));
    }
    for addr in relay_addrs {
        filter.push_str(&format!(" and ip.DstAddr != {addr}"));
    }
    filter
}

/// The inbound mirror of [`build_capture_filter`], for the sniff-only
/// shadow-direct handle: same capture scope and the same relay exclusions,
/// mirrored to the reply direction.
fn build_shadow_filter(policy: CapturePolicy, relay_addrs: &[Ipv4Addr]) -> String {
    let mut filter = String::from("udp and inbound");
    if let CapturePolicy::PortRange { lo, hi } = policy {
        if lo == hi {
            filter.push_str(&format!(" and udp.SrcPort == {lo}"));
        } else {
            filter.push_str(&format!(" and udp.SrcPort >= {lo} and udp.SrcPort <= {hi}"));
        }
    }
    for addr in relay_addrs {
        filter.push_str(&format!(" and ip.SrcAddr != {addr}"));
    }
    filter
}

/// Every address the client's own tunnel traffic can be addressed to: the
/// configured relay plus whatever multipath destinations are active right now.
///
/// The process-wide filter would otherwise capture the client's own QUIC
/// control and keepalive packets and relay them back into the tunnel, which is
/// exactly the self-interception the relay-address exclusion exists to prevent.
fn relay_addresses(config_proxy: SocketAddrV4) -> Vec<Ipv4Addr> {
    let (dests, n) = crate::session::send_destinations(config_proxy);
    let mut addrs = vec![*config_proxy.ip()];
    for d in dests.iter().take(n) {
        if !addrs.contains(d.ip()) {
            addrs.push(*d.ip());
        }
    }
    addrs
}

// ─────────────────────────────────────────────────────────────────────────────
//  Userspace attribution (gated: the port-scoped half needs the tracker)
// ─────────────────────────────────────────────────────────────────────────────

/// The per-packet outbound action the intercept thread takes.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutboundDecision {
    /// Relay: drop the original and forward the payload to the relay.
    Tunnel(SocketAddrV4),
    /// Re-inject the original packet unchanged on the direct path.
    Reinject,
    /// Port-scoped mode only: the locked server went stale; re-detect.
    Redetect,
}

/// Process mode's attribution state: the bounded map plus the cap warning that
/// was last surfaced, so the warning is emitted once per new tunneled count
/// rather than once per packet.
///
/// Ownership is decided two ways, and both are needed. [`OwnerCache`] answers
/// "does the target PID own the socket this packet left from?" by asking the OS
/// for the live binding table - that is the only answer available for a flow
/// that opened before this interceptor did, which is the normal case when the
/// app is started while the game is already running. The [`FlowMap`] then holds
/// the per-flow state (tunneled or not, freshness, the cap). A packet whose
/// local port the table attributes to the target is promoted into the map
/// before the map is consulted, so a pre-existing flow is relayed from its
/// first captured datagram instead of waiting for a seed that UDP cannot
/// supply.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
struct ProcessAttributor {
    map: FlowMap,
    owners: OwnerCache,
    target_pid: Option<u32>,
    warned_tunneled: usize,
    counts: AttributionCounts,
    /// The tallies as last logged, so a quiet tick logs nothing.
    reported: AttributionCounts,
    last_report: Option<Instant>,
    /// While nothing has matched yet, a few (source port, owner pids) pairs are
    /// collected and reported once. Diagnostic only, bounded, and silenced for
    /// the rest of the run after it fires.
    unmatched_samples: Vec<(u16, Vec<u32>)>,
    /// Whether the no-match diagnosis has already been reported.
    diagnosed: bool,
}

/// Per-run tallies for the process-mode path.
///
/// These exist because silence is the worst failure mode an interceptor has: a
/// user who picked a process and sees no relayed traffic cannot tell "nothing
/// has been sent yet" from "your traffic is not being seen at all". The values
/// are logged once per [`ATTRRIBUTION_REPORT_INTERVAL`] while any of them move.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AttributionCounts {
    /// Packets captured by the static filter.
    captured: u64,
    /// Captured packets whose local port the owner table attributed to the target.
    owned_by_target: u64,
    /// Packets routed into the tunnel.
    tunneled: u64,
    /// Packets sent back out on the direct path (excluded, unattributed, or
    /// over the cap). Fail-open, never a drop.
    direct: u64,
}

/// How often the tallies are surfaced while they are still moving.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
const ATTRRIBUTION_REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// The intercept thread's attribution state machine: one variant per capture
/// policy.
///
/// Process mode attributes every captured packet in userspace through the
/// bounded [`FlowMap`]; the built-in path keeps its single-server tracker.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
enum Attributor {
    /// A picked process: userspace attribution, capped and fail-open.
    ///
    /// Boxed because this arm carries the flow map, the owner cache and the
    /// tallies, while the built-in path is one tracker; without the box every
    /// stack frame holding an `Attributor` would be sized by the larger arm.
    Process(Box<ProcessAttributor>),
    /// The built-in ports path, unchanged.
    Ports(ServerTracker),
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl Attributor {
    /// One captured packet in, one route out.
    ///
    /// Fail-open at the call site: every arm either relays or re-injects. The
    /// process arm decides ownership against the OS's live binding table (see
    /// [`ProcessAttributor`]), then consults the flow map for per-flow state.
    fn decide(
        &mut self,
        src: SocketAddrV4,
        dst: SocketAddrV4,
        relay_addrs: &[Ipv4Addr],
        now: Instant,
        payload_len: usize,
    ) -> OutboundDecision {
        match self {
            Self::Process(state) => {
                state.counts.captured += 1;

                // Ownership first: an unfiltered capture-all handle sees every
                // process's UDP, so the packet is only ours if the OS says the
                // target owns the local port it came from. This is what makes a
                // flow that predates the handle work; a seeded route list
                // cannot, because the UDP table carries no remote endpoint.
                //
                // The hint is deliberately weaker than a route: two processes
                // can bind one port, so a hinted flow starts unlocked and must
                // pass the map's own debounce before it can take a tunnel slot.
                let owned = match state.target_pid {
                    Some(pid) => state.owners.table(now).owns(src.port(), pid),
                    // No PID resolved at start: fall back to the seeded map
                    // rather than refusing every packet.
                    None => true,
                };

                // While nothing has matched, collect a few unmatched source ports
                // with their real owners and report them ONCE. A report of "I
                // targeted my game and nothing happened" is answered by this line:
                // it shows whether the captured ports belong to the target (a
                // matching bug) or to nobody the table knows (the wrong traffic is
                // being captured). Bounded to one message per run.
                if !owned && state.counts.owned_by_target == 0 && !state.diagnosed {
                    let pids = state.owners.table(now).pids_for_port(src.port()).to_vec();
                    state.unmatched_samples.push((src.port(), pids));
                    if state.unmatched_samples.len() >= 8 {
                        let samples: Vec<String> = state
                            .unmatched_samples
                            .drain(..)
                            .map(|(p, pids)| format!("{p}->{pids:?}"))
                            .collect();
                        tracing::info!(
                            "🔍 process mode: no packet has matched the target yet; captured source \
                             ports and their real owners: {}. Empty owner lists mean those ports \
                             belong to neither the target nor any live socket; the target's ports \
                             appearing here would instead point at a matching fault.",
                            samples.join(" ")
                        );
                        state.diagnosed = true;
                    }
                }
                if owned {
                    state.counts.owned_by_target += 1;
                    if let Some(pid) = state.target_pid {
                        state.map.hint_port(src.port(), pid);
                    }
                }

                let route = route_outbound(&mut state.map, src, dst, relay_addrs, now, payload_len);
                match route {
                    OutboundRoute::Tunnel => state.counts.tunneled += 1,
                    OutboundRoute::Excluded | OutboundRoute::Reinject(_) => {
                        state.counts.direct += 1
                    }
                }

                // The cap lives in the map (`super::flows`): at most
                // MAX_TUNNELED_FLOWS tunneled, warned from WARN_TUNNELED_FLOWS,
                // and over the cap the flow goes direct and is counted
                // UNATTRIBUTED. Surface the warning once per new count.
                let report = state.map.report();
                match &report.warning {
                    Some(warning) if warning.tunneled != state.warned_tunneled => {
                        tracing::warn!("⚠️ {}", warning.message);
                        state.warned_tunneled = warning.tunneled;
                    }
                    Some(_) => {}
                    None => state.warned_tunneled = 0,
                }

                // Surface the tallies while they are moving, so a run can never
                // end in silence. A quiet tick logs nothing.
                let due = match state.last_report {
                    Some(last) => now.duration_since(last) >= ATTRRIBUTION_REPORT_INTERVAL,
                    None => true,
                };
                if due && state.counts.captured > 0 {
                    if state.counts != state.reported {
                        tracing::info!(
                            "📊 process mode: captured {} | owned by target {} | tunneled {} | direct {} | flows {}",
                            state.counts.captured,
                            state.counts.owned_by_target,
                            state.counts.tunneled,
                            state.counts.direct,
                            report.tunneled,
                        );
                        state.reported = state.counts;
                    }
                    state.last_report = Some(now);
                }

                match route {
                    OutboundRoute::Tunnel => OutboundDecision::Tunnel(dst),
                    OutboundRoute::Excluded | OutboundRoute::Reinject(_) => {
                        OutboundDecision::Reinject
                    }
                }
            }
            Self::Ports(tracker) => match tracker.observe(dst, now, payload_len) {
                Decision::Tunnel(server) => OutboundDecision::Tunnel(server),
                Decision::ResetToDetection => OutboundDecision::Redetect,
                Decision::ShadowDirect | Decision::PassThrough | Decision::StartDetection => {
                    OutboundDecision::Reinject
                }
            },
        }
    }
}

/// Build the attributor the intercept thread drives for this policy.
///
/// Process mode seeds the map from the scanner's live routes for the target *and*
/// installs a live owner-lookup cache. Both are needed and neither is redundant:
/// a seeded route locks immediately, but the scanner can only report sockets it
/// could already see, and the UDP table carries no remote endpoint - so a flow
/// that opened before this interceptor did is invisible to the seed list. The
/// owner table answers for exactly that case by mapping a captured packet's local
/// port to its owning PID, which the map then accepts through its own debounce.
/// A flow neither source attributes is UNATTRIBUTED and goes direct, visibly -
/// fail-open, never a drop.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn build_attributor(
    policy: CapturePolicy,
    target_pid: Option<u32>,
    seed_routes: &[Route],
    seeds: &[SocketAddrV4],
    now: Instant,
) -> Attributor {
    match policy {
        CapturePolicy::ProcessAll => {
            let mut map = FlowMap::new(super::flows::FlowMapConfig::default(), target_pid);
            // With no target PID the map accepts any attributed process, so the
            // pid handed to `seed` only has to be acceptable.
            map.seed(target_pid.unwrap_or(0), seed_routes, now);
            Attributor::Process(Box::new(ProcessAttributor {
                map,
                owners: OwnerCache::default(),
                target_pid,
                warned_tunneled: 0,
                counts: AttributionCounts::default(),
                reported: AttributionCounts::default(),
                last_report: None,
                unmatched_samples: Vec::new(),
                diagnosed: false,
            }))
        }
        CapturePolicy::PortRange { .. } => {
            let mut tracker = ServerTracker::new(TrackerConfig::default());
            tracker.seed(seeds, now);
            Attributor::Ports(tracker)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  TrafficInterceptor - full implementation (gated on feature + platform)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl TrafficInterceptor for WinDivertInterceptor {
    fn platform_name(&self) -> &'static str {
        "WinDivert"
    }

    fn check_availability(&self) -> Result<(), String> {
        // Check that WinDivert64.sys and WinDivert.dll are next to the exe.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        let sys = exe_dir.join("WinDivert64.sys");
        let dll = exe_dir.join("WinDivert.dll");
        if !sys.exists() {
            return Err(format!(
                "WinDivert64.sys not found at {}\n\
                 Download WinDivert 2.x from https://reqrypt.org/windivert.html",
                sys.display()
            ));
        }
        if !dll.exists() {
            return Err(format!("WinDivert.dll not found at {}", dll.display()));
        }
        Ok(())
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        use bytes::BytesMut;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use std::collections::HashMap;
        use tokio::net::UdpSocket;

        let config_proxy = config.proxy_addr;
        let dynamic_server = config.dynamic_server;
        let route_set: Vec<SocketAddrV4> = config.initial_routes.iter().map(|r| r.remote).collect();
        let seeds = effective_seeds(dynamic_server, &route_set);
        let pre_known_server: Option<SocketAddrV4> = seeds.first().copied();

        // ── Policy, attribution seeds and the ONE static filter ───────────
        //
        // The capture policy is derived in exactly one place (`capture_policy`
        // in `mod.rs`): a picked process means capture the whole process's UDP
        // and attribute it in userspace; the built-in path keeps its UDP port
        // range. The filter built below is the only one this backend ever opens
        // and the handle it opens lives for the whole interception: no per-flow
        // rebuild, no swap on flow change, no close/reopen on flow events.
        //
        // `processId` is deliberately absent: the network layer cannot match it
        // (only the Flow/Socket layers can), so the PID identifies the target
        // through the userspace attribution below instead.
        let policy = super::capture_policy(&config);
        let relay_addrs = relay_addresses(config_proxy);
        let out_filter = build_capture_filter(policy, &relay_addrs);
        tracing::info!("🔀 WinDivert intercept filter ({policy:?}): {out_filter}");

        // The effective attribution seeds: the scanner's live routes, filtered
        // by the dynamic-server rule. A rotating title gets none, so its
        // entries earn their lock through the map's own debounce.
        let seed_routes: Vec<Route> = config
            .initial_routes
            .iter()
            .filter(|r| seeds.contains(&r.remote))
            .cloned()
            .collect();
        // The target's PID is resolved HERE against the live process table, per
        // the contract on `ProcessTarget::pid`: a name is the identity (it
        // survives a relaunch) and the PID is the hint that this overwrites. A
        // caller that picks a process by name - every GUI path does - leaves
        // `pid` as `None`, and without this resolution the owner lookup below
        // would have no PID to match against and every packet would fall to the
        // direct path while reporting nothing wrong.
        let target_pid = match config.process_target.as_ref() {
            Some(target) if target.is_set() => match target.name() {
                Some(name) => crate::interceptor::process_scanner::find_pid_by_name(name),
                None => target.pid(),
            },
            _ => None,
        };
        if let Some(target) = config.process_target.as_ref() {
            match target_pid {
                Some(pid) => tracing::info!(
                    "🎯 process target '{}' resolved to PID {pid}",
                    target.describe()
                ),
                None => tracing::warn!(
                    "⚠️ process target '{}' is not running; capture stays broad and \
                     nothing will be attributed until it starts",
                    target.describe()
                ),
            }
        }

        // Open intercept handle (captures matching outbound packets).
        tracing::info!("⚡ Opening WinDivert intercept handle...");
        let wd_intercept = match OwnedHandle::open(
            &out_filter,
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new(),
        ) {
            Ok(h) => {
                tracing::info!("✅ WinDivert intercept handle opened successfully");
                Arc::new(h)
            }
            Err(e) => {
                log_open_failure("intercept", &e);
                return Err(anyhow::anyhow!(
                    "WinDivert open failed (need Admin + WinDivert64.sys loaded): {e}"
                ));
            }
        };

        // Open inject handle (no filter - write-only for injecting inbound spoofs).
        tracing::info!("⚡ Opening WinDivert inject handle...");
        let wd_inject =
            match OwnedHandle::open("false", WinDivertLayer::Network, 0, WinDivertFlags::new()) {
                Ok(h) => {
                    tracing::info!("✅ WinDivert inject handle opened successfully");
                    Arc::new(h)
                }
                Err(e) => {
                    log_open_failure("inject", &e);
                    // Close the already-open intercept handle before returning.
                    let _ = wd_intercept.close();
                    return Err(anyhow::anyhow!("WinDivert inject handle failed: {e}"));
                }
            };

        // Open a shadow-direct inbound handle. It is a sniff handle (copy, never
        // divert), so it observes replies from a tracked game server to a
        // sampled game packet without disrupting either direction. It mirrors
        // the outbound capture scope on SrcPort and the read loop additionally
        // matches the source against the tracked server. Non-fatal: shadow
        // sampling refines telemetry, it is never a reason to fail interception.
        let shadow_filter = build_shadow_filter(policy, &relay_addrs);
        tracing::info!(
            "🔀 WinDivert shadow-direct inbound filter: {}",
            shadow_filter
        );
        let wd_shadow: Option<Arc<OwnedHandle>> = match OwnedHandle::open(
            &shadow_filter,
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new().set_sniff(),
        ) {
            Ok(h) => {
                tracing::info!("✅ WinDivert shadow-direct inbound handle opened");
                Some(Arc::new(h))
            }
            Err(e) => {
                tracing::warn!(
                    "WinDivert shadow-direct inbound handle failed (shadow sampling disabled): {e}"
                );
                None
            }
        };

        // All handles are open: any later setup error must release them.
        let mut setup_guard = HandleShutdownGuard::new(
            Arc::clone(&wd_intercept),
            Arc::clone(&wd_inject),
            wd_shadow.clone(),
        );

        // Shared state
        let counters = Arc::new(InterceptorCounters::default());
        let running = Arc::new(AtomicBool::new(true));
        tracing::info!("✅ Shared state created");

        // Pre-seed detected_server if we already know it from the route scanner.
        if let Some(s) = pre_known_server {
            if let Ok(mut g) = counters.detected_server.lock() {
                *g = Some(s);
            }
        }

        // Interface address cache
        let if_addr_cache: Arc<std::sync::Mutex<Option<WINDIVERT_ADDRESS>>> =
            Arc::new(std::sync::Mutex::new(None));

        // Channels
        let (intercept_tx, mut intercept_rx) =
            tokio::sync::mpsc::channel::<(SocketAddrV4, SocketAddrV4, Vec<u8>)>(512);
        let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(512);
        tracing::info!("✅ Channels created");

        // Each owner thread sends one ack when it can no longer touch its
        // WinDivert handle, so `stop_and_wait` can close deterministically.
        // The tunnel task also removes the firewall rule, so teardown does not
        // finish before the rule is gone. The shadow inbound thread acks only
        // when its sniff handle is open.
        let ack_count = if wd_shadow.is_some() { 4 } else { 3 };
        let (ack_tx, teardown_ack) = TeardownAck::new(ack_count);

        // ── Shutdown signal ──────────────────────────────────────────────
        // Keep tokio::sync::oneshot for InterceptorHandle compatibility, but
        // use tokio::runtime::Handle::block_on inside a std::thread so we avoid
        // the tokio::spawn(…) panic when start() is called from a non-tokio thread
        // (e.g. the egui main loop on the GUI).
        tracing::info!("🔌 Creating shutdown signal...");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        tracing::info!("✅ Shutdown signal created");
        tracing::info!("🔌 Spawning shutdown thread...");
        {
            let running = Arc::clone(&running);
            let rt_handle = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                tracing::info!("🎯 Shutdown thread started, waiting for signal...");
                rt_handle.block_on(async {
                    let _ = shutdown_rx.await;
                });
                tracing::info!("🛑 Shutdown signal received");
                running.store(false, Ordering::Relaxed);
            });
        }
        tracing::info!("✅ Shutdown thread spawned");

        // ── Intercept thread (blocking) ───────────────────────────────────
        //
        // Reads intercepted outbound packets from WinDivert, auto-detects the
        // game server, then forwards packets to the async tunnel task via mpsc.
        {
            let wd_ic = Arc::clone(&wd_intercept);
            let counters_ic = Arc::clone(&counters);
            let if_cache_ic = Arc::clone(&if_addr_cache);
            let itx = intercept_tx;
            let running_ic = Arc::clone(&running);
            let seeds_ic = seeds;
            let seed_routes_ic = seed_routes;
            let relay_addrs_ic = relay_addrs;
            let ack_ic = ack_tx.clone();

            tracing::info!("🚀 Spawning intercept thread...");
            tokio::task::spawn_blocking(move || {
                tracing::info!("🎯 Intercept thread started");
                const RECV_MAX_RETRIES: u32 = 5;
                const RECV_BACKOFF_BASE: Duration = Duration::from_millis(10);
                const RECV_BACKOFF_MAX: Duration = Duration::from_millis(400);

                // The userspace attributor: the bounded FlowMap for a picked
                // process, the single-server tracker for the built-in path.
                let mut attributor = build_attributor(
                    policy,
                    target_pid,
                    &seed_routes_ic,
                    &seeds_ic,
                    Instant::now(),
                );
                let mut reported_server: Option<SocketAddrV4> = pre_known_server;
                let mut backoff =
                    RecvBackoff::new(RECV_MAX_RETRIES, RECV_BACKOFF_BASE, RECV_BACKOFF_MAX);
                let mut recv_buf = vec![0u8; 65535];

                loop {
                    if !running_ic.load(Ordering::Relaxed) {
                        break;
                    }

                    match wd_ic.recv(&mut recv_buf) {
                        Ok((len, addr)) => {
                            backoff.on_success();
                            let data = &recv_buf[..len];

                            // Cache the interface index on the first outbound
                            // packet. The copy is mutated for injection; `addr`
                            // stays untouched so re-injection keeps the
                            // original outbound direction and checksums.
                            {
                                let mut guard = if_cache_ic
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                if guard.is_none() {
                                    let mut cached = addr;
                                    cached.set_outbound(false);
                                    cached.set_ipchecksum(false);
                                    cached.set_udpchecksum(false);
                                    // SAFETY: [Category 8 - FFI] `addr` came from
                                    // a successful network-layer `WinDivertRecv`,
                                    // so the driver initialized the `Network`
                                    // union member. This reads one plain `u32`
                                    // from that `#[repr(C)]` union.
                                    let if_idx = unsafe { cached.union_field.Network.interface_id };
                                    tracing::info!(
                                        "🔗 WinDivert: cached inject interface IfIdx={if_idx}"
                                    );
                                    *guard = Some(cached);
                                }
                            }

                            let parsed =
                                parse_ipv4_udp(data).map(|(src, dst, pl)| (src, dst, pl.to_vec()));

                            match parsed {
                                Some((game_src, game_dst, payload)) => {
                                    // Userspace attribution: the one decision
                                    // point. Process mode looks the flow up in
                                    // the bounded FlowMap (exclusions first, so
                                    // an excluded destination never takes a
                                    // tunnel slot); port-scoped mode keeps the
                                    // single-server tracker.
                                    let decision = attributor.decide(
                                        game_src,
                                        game_dst,
                                        &relay_addrs_ic,
                                        Instant::now(),
                                        payload.len(),
                                    );

                                    // Publish the server this packet is bound
                                    // for before the decision can become a
                                    // shadow-direct sample: the shadow inbound
                                    // thread pairs replies against it.
                                    if let OutboundDecision::Tunnel(server) = decision {
                                        if reported_server != Some(server) {
                                            reported_server = Some(server);
                                            if let Ok(mut g) = counters_ic.detected_server.lock() {
                                                *g = Some(server);
                                            }
                                            tracing::info!("🔍 Locked game server: {}", server);
                                        }
                                    }

                                    // The shadow-direct exemption, kept: when the
                                    // sampler is due for this server, this packet
                                    // is re-injected unchanged on the direct path
                                    // instead of relayed, and the sniff inbound
                                    // handle times the reply.
                                    let decision = match decision {
                                        OutboundDecision::Tunnel(server)
                                            if crate::latency::record_shadow_outbound(
                                                *server.ip(),
                                            ) =>
                                        {
                                            OutboundDecision::Reinject
                                        }
                                        other => other,
                                    };

                                    match decision {
                                        OutboundDecision::Tunnel(_) => {
                                            counters_ic
                                                .packets_intercepted
                                                .fetch_add(1, Ordering::Relaxed);
                                            counters_ic
                                                .bytes_intercepted
                                                .fetch_add(payload.len() as u64, Ordering::Relaxed);
                                            if itx
                                                .blocking_send((game_src, game_dst, payload))
                                                .is_err()
                                            {
                                                break;
                                            }
                                        }
                                        OutboundDecision::Reinject => {
                                            // Re-inject the game's OWN packet
                                            // UNCHANGED on the direct path. No
                                            // synthetic packet and no payload
                                            // rewrite: the anti-cheat safety
                                            // property. This is also where an
                                            // unattributed, over-cap or excluded
                                            // packet goes, so nothing is silently
                                            // dropped or relayed.
                                            let _ = wd_ic.send(data, &addr);
                                        }
                                        OutboundDecision::Redetect => {
                                            tracing::info!("🔄 Locked server stale - re-detecting");
                                            reported_server = None;
                                            if let Ok(mut g) = counters_ic.detected_server.lock() {
                                                *g = None;
                                            }
                                            if let Ok(mut c) = if_cache_ic.lock() {
                                                *c = None;
                                            }
                                            let _ = wd_ic.send(data, &addr);
                                        }
                                    }
                                }
                                None => {
                                    // Non-IPv4/UDP - re-inject unchanged.
                                    let _ = wd_ic.send(data, &addr);
                                }
                            }
                        }
                        Err(e) => {
                            let raw = e.raw_os_error().unwrap_or(0);
                            if recv_should_break(running_ic.load(Ordering::Relaxed), raw) {
                                break;
                            }
                            counters_ic.errors.fetch_add(1, Ordering::Relaxed);
                            match backoff.on_failure() {
                                Some(delay) => {
                                    tracing::warn!(
                                        "WinDivert recv error (retry {}/{} in {:?}): {e}",
                                        backoff.consecutive_failures(),
                                        RECV_MAX_RETRIES,
                                        delay
                                    );
                                    std::thread::sleep(delay);
                                }
                                None => {
                                    let msg = format!(
                                        "WinDivert recv failed {} times consecutively: {e}",
                                        backoff.consecutive_failures()
                                    );
                                    tracing::error!("{msg}");
                                    if let Ok(mut g) = counters_ic.last_error.lock() {
                                        *g = Some(msg);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                // The owner closes the handle only after every owner thread
                // sends this ack, so the thread must not touch it afterwards.
                let _ = ack_ic.send(());
                tracing::info!("WinDivert intercept thread exiting");
            });
        }
        tracing::info!("✅ Intercept thread spawned");

        // ── Inject thread (blocking) ─────────────────────────────────────
        {
            tracing::info!("🚀 Spawning inject thread...");
            let wd_inj = Arc::clone(&wd_inject);
            let counters_inj = Arc::clone(&counters);
            let if_cache_inj = Arc::clone(&if_addr_cache);
            let running_inj = Arc::clone(&running);
            let ack_inj = ack_tx.clone();

            tokio::task::spawn_blocking(move || {
                tracing::info!("🎯 Inject thread started");
                loop {
                    match inject_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(raw) => {
                            let addr = {
                                let guard = if_cache_inj
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                match guard.as_ref() {
                                    Some(a) => *a,
                                    None => WINDIVERT_ADDRESS::default(),
                                }
                            };
                            match wd_inj.send(&raw, &addr) {
                                Ok(_) => {
                                    counters_inj
                                        .packets_injected
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    tracing::warn!("WinDivert inject error: {e}");
                                    counters_inj.errors.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if !running_inj.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                // The owner closes the handle only after every owner thread
                // sends this ack, so the thread must not touch it afterwards.
                let _ = ack_inj.send(());
                tracing::info!("WinDivert inject thread exiting");
            });
        }
        tracing::info!("✅ Inject thread spawned");

        // ── Shadow-direct inbound thread (blocking) ───────────────────────
        //
        // Reads the sniff handle and records a direct application RTT when a
        // reply comes from the tracked game server. The sniff handle never
        // diverts, so the game still receives every reply.
        if let Some(wd_sh) = wd_shadow.as_ref() {
            let wd_sh = Arc::clone(wd_sh);
            let counters_sh = Arc::clone(&counters);
            let running_sh = Arc::clone(&running);
            let ack_sh = ack_tx.clone();

            tracing::info!("🚀 Spawning shadow-direct inbound thread...");
            tokio::task::spawn_blocking(move || {
                let mut recv_buf = vec![0u8; 65535];
                loop {
                    if !running_sh.load(Ordering::Relaxed) {
                        break;
                    }
                    match wd_sh.recv(&mut recv_buf) {
                        Ok((len, addr)) => {
                            // Skip packets our own inject handle spoofed for the
                            // tunnelled path: only a genuine wire reply proves
                            // the direct path's RTT.
                            if addr.impostor() {
                                continue;
                            }
                            let Some((src, _dst, _payload)) = parse_ipv4_udp(&recv_buf[..len])
                            else {
                                continue;
                            };
                            let tracked = *counters_sh
                                .detected_server
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            if tracked.filter(|server| server.ip() == src.ip()).is_some() {
                                crate::latency::record_shadow_inbound(*src.ip());
                            }
                        }
                        Err(e) => {
                            let raw = e.raw_os_error().unwrap_or(0);
                            if recv_should_break(running_sh.load(Ordering::Relaxed), raw) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                let _ = ack_sh.send(());
                tracing::info!("WinDivert shadow-direct thread exiting");
            });
        }
        tracing::info!("✅ Shadow-direct inbound thread spawned");

        // ── Async tunnel task ─────────────────────────────────────────────
        {
            tracing::info!("🚀 Creating tunnel UDP socket...");
            let counters_t = Arc::clone(&counters);
            let counters_ka = Arc::clone(&counters);
            let _running_t = Arc::clone(&running);

            let tunnel_std = std::net::UdpSocket::bind("0.0.0.0:0")
                .map_err(|e| anyhow::anyhow!("Tunnel socket bind failed: {e}"))?;
            tunnel_std.set_nonblocking(true)?;
            let tunnel_socket = Arc::new(UdpSocket::from_std(tunnel_std)?);
            tracing::info!("✅ Tunnel UDP socket created");
            let tunnel_port = tunnel_socket
                .local_addr()
                .map_err(|e| anyhow::anyhow!("Cannot get tunnel port: {e}"))?
                .port();
            tracing::info!("🔌 Tunnel socket bound to port {}", tunnel_port);

            // Add Windows Firewall rule so proxy responses reach our tunnel socket.
            tracing::info!("🔓 Adding firewall rule for port {}...", tunnel_port);
            add_fw_rule(tunnel_port);
            tracing::info!("✅ Firewall rule added");

            tracing::info!("⚡ WinDivert active - tunnel socket port {}", tunnel_port);

            // Keepalive timestamps
            let ka_ts: Arc<tokio::sync::Mutex<HashMap<u16, Instant>>> =
                Arc::new(tokio::sync::Mutex::new(HashMap::new()));

            // Keepalive sender task
            {
                let ts = Arc::clone(&tunnel_socket);
                let ka = Arc::clone(&ka_ts);
                let running_ka = Arc::clone(&running);
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(Duration::from_secs(5));
                    let mut seq: u16 = 60000;
                    while running_ka.load(Ordering::Relaxed) {
                        ticker.tick().await;
                        let now_us = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;
                        let hdr = lightspeed_protocol::TunnelHeader::keepalive(seq, now_us)
                            .with_session_token(crate::session::session_token());
                        if ts
                            .send_to(
                                &hdr.encode_to_array(),
                                crate::session::current_proxy().unwrap_or(config_proxy),
                            )
                            .await
                            .is_ok()
                        {
                            let mut m = ka.lock().await;
                            m.insert(seq, Instant::now());
                            m.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                        }
                        seq = seq.wrapping_add(1);
                        let _ = counters_ka.packets_from_proxy.load(Ordering::Relaxed);
                        // touch to avoid dead_code warning
                    }
                });
            }

            // Main async loop: forward intercepted packets to proxy, inject responses.
            let fec_enabled = config.fec_enabled;
            let fec_k = config.fec_k;
            let adaptive_cfg = config.adaptive_fec;
            let fec_active = fec_enabled || adaptive_cfg.enabled;
            let bypass_config = config.bypass;
            let running_loop = Arc::clone(&running);
            let ack_loop = ack_tx.clone();

            tokio::spawn(async move {
                let mut fec_gate = InlineFecGate::new(adaptive_cfg, fec_k);
                let mut fec_encoder = if fec_active {
                    Some(lightspeed_protocol::FecEncoder::new(fec_gate.block_k()))
                } else {
                    None
                };
                let mut fec_decoders: std::collections::HashMap<
                    std::net::SocketAddr,
                    lightspeed_protocol::FecDecoder,
                > = std::collections::HashMap::new();

                // The steady-state do-no-harm gate. WinDivert is the backend
                // that re-injects the game's own packets onto the direct path,
                // so it is the one that produces same-server shadow-direct
                // samples for the gate to compare. Evaluated on a timer rather
                // than per packet; the gate degrades to measurement-only.
                let mut gate =
                    super::bypass::BypassGate::new(bypass_config, Arc::clone(&counters_t));
                let mut eval_timer = tokio::time::interval(super::bypass::EVAL_INTERVAL);
                eval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                let mut seq: u16 = 0;
                let mut buf = vec![0u8; 65535];
                let mut game_src_learned: Option<SocketAddrV4> = None;
                let mut active_server: Option<SocketAddrV4> = pre_known_server;

                loop {
                    if !running_loop.load(Ordering::Relaxed) {
                        break;
                    }

                    tokio::select! {
                        biased;

                        // Intercepted game → proxy
                        maybe = intercept_rx.recv() => {
                            let (game_src, game_dst, payload) = match maybe {
                                Some(p) => p,
                                None => break,
                            };

                            if game_src_learned.is_none() {
                                tracing::info!("🎮 Game client at {}", game_src);
                            }
                            game_src_learned = Some(game_src);
                            active_server = Some(game_dst);

                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_micros() as u32;

                            if let crate::tunnel::budget::PayloadFit::OverBudget {
                                payload_len,
                                budget,
                            } = crate::tunnel::budget::classify(payload.len(), fec_encoder.is_some())
                            {
                                counters_t.payloads_over_budget.fetch_add(1, Ordering::Relaxed);
                                tracing::warn!(
                                    payload_len = payload_len,
                                    budget = budget,
                                    "WinDivert interceptor: forwarding oversized game payload; the kernel may fragment it"
                                );
                            }

                            if let Some(ref mut enc) = fec_encoder {
                                let block_id = enc.block_id();
                                let index = enc.current_index();
                                let k_size = enc.k_size();
                                let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts, game_src, game_dst)
                                    .with_session_token(crate::session::session_token());
                                let fh = FecHeader::data(block_id, index, k_size);
                                let mut buf2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
                                buf2.extend_from_slice(&hdr.encode_to_array());
                                fh.encode(&mut buf2);
                                buf2.extend_from_slice(&payload);
                                let parity = enc.add_packet(&payload);
                                // Same gate as the relay: drop this block's parity
                                // while the link is clean.
                                let emit_parity = fec_gate.emit_parity();
                                if parity.is_some() {
                                    fec_gate.record_block(emit_parity);
                                }
                                counters_t
                                    .adaptive_parity_ratio_bp
                                    .store(fec_gate.parity_ratio_bp(), Ordering::Relaxed);
                                crate::latency::record_outbound(*game_dst.ip());
                                let _ = crate::tunnel::transport::send_datagram(
                                    &tunnel_socket,
                                    &buf2,
                                    crate::session::current_proxy().unwrap_or(config_proxy),
                                )
                                .await;
                                if let Some(pb) = parity.filter(|_| emit_parity) {
                                    let ps = seq.wrapping_add(1);
                                    let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts, game_src, game_dst)
                                        .with_session_token(crate::session::session_token());
                                    let pf = FecHeader::parity(block_id, k_size);
                                    let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                    pb2.extend_from_slice(&ph.encode_to_array());
                                    pf.encode(&mut pb2);
                                    pb2.extend_from_slice(&pb);
                                    let _ = crate::tunnel::transport::send_datagram(
                                        &tunnel_socket,
                                        &pb2,
                                        crate::session::current_proxy().unwrap_or(config_proxy),
                                    )
                                    .await;
                                    seq = seq.wrapping_add(1);
                                }
                            } else {
                                let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts, game_src, game_dst)
                                    .with_session_token(crate::session::session_token());
                                let pkt = hdr.encode_with_payload(&payload);
                                crate::latency::record_outbound(*game_dst.ip());
                                let (dests, n) = crate::session::send_destinations(config_proxy);
                                for d in dests.iter().take(n) {
                                    let _ = crate::tunnel::transport::send_datagram(
                                        &tunnel_socket,
                                        &pkt,
                                        *d,
                                    )
                                    .await;
                                }
                            }
                            seq = seq.wrapping_add(1);
                        }

                        // Proxy response → inject as spoofed server packet
                        recv = tokio::time::timeout(Duration::from_millis(50), tunnel_socket.recv_from(&mut buf)) => {
                            let (len, src_addr) = match recv {
                                Ok(Ok(r)) => r,
                                _ => continue,
                            };

                            if !crate::session::is_known_proxy(src_addr, config_proxy) {
                                continue;
                            }

                            counters_t.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                            let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                                Ok(r) => r,
                                Err(_) => continue,
                            };

                            if !header.is_keepalive() {
                                crate::latency::record_inbound(*header.orig_src_addr().ip());
                            }

                            if header.is_keepalive() {
                                continue; // keepalive echo - RTT measured by keepalive task
                            }

                            let game_src = match game_src_learned {
                                Some(gs) => gs,
                                None => continue,
                            };

                            let mut recovered = false;
                            let game_data: Option<bytes::Bytes> = if header.has_fec() {
                                if payload.len() < FEC_HEADER_SIZE { continue; }
                                let mut sl: &[u8] = &payload[..FEC_HEADER_SIZE];
                                let fh = match lightspeed_protocol::FecHeader::decode(&mut sl) {
                                    Some(h) => h,
                                    None => continue,
                                };
                                let data = &payload[FEC_HEADER_SIZE..];
                                let dec = fec_decoders.entry(src_addr).or_default();
                                if fh.is_parity() {
                                    let recovered_pkt = dec
                                        .receive_parity(&fh, bytes::Bytes::copy_from_slice(data))
                                        .map(|(_, r)| r);
                                    recovered = recovered_pkt.is_some();
                                    if recovered {
                                        crate::telemetry::paths::record_relay_recovery(src_addr);
                                    }
                                    recovered_pkt
                                } else {
                                    let b = bytes::Bytes::copy_from_slice(data);
                                    dec.receive_data(&fh, b.clone());
                                    if crate::session::multipath_record_response(
                                        header.sequence,
                                        src_addr,
                                        0,
                                    ) {
                                        None
                                    } else {
                                        Some(b)
                                    }
                                }
                            } else if crate::session::multipath_record_response(
                                header.sequence,
                                src_addr,
                                0,
                            ) {
                                None
                            } else {
                                Some(bytes::Bytes::copy_from_slice(payload))
                            };

                            {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_micros() as u32;
                                fec_gate.observe_response(recovered, header.timestamp_us, now);
                                counters_t
                                    .adaptive_parity_ratio_bp
                                    .store(fec_gate.parity_ratio_bp(), Ordering::Relaxed);
                            }

                            if let Some(data) = game_data {
                                if !data.is_empty() {
                                    let spoof_src = match active_server {
                                        Some(s) => s,
                                        None => continue,
                                    };
                                    let raw = build_ipv4_udp(spoof_src, game_src, &data);
                                    counters_t.bytes_injected.fetch_add(data.len() as u64, Ordering::Relaxed);
                                    if inject_tx.try_send(raw).is_err() {
                                        tracing::warn!("Inject channel full - dropping response");
                                        counters_t.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }

                        // Steady-state bypass evaluation. Counts and logs the
                        // decision; it never enacts one, because a Direct here
                        // would mean tearing a live divert off the tunnel.
                        _ = eval_timer.tick() => {
                            if let Some(server) = active_server.filter(|_| !gate.is_disabled()) {
                                let _ = super::bypass::steady_state_step(
                                    &mut gate,
                                    server,
                                    Instant::now(),
                                    true,
                                    fec_active,
                                );
                            }
                        }
                    }
                }

                // Cleanup
                remove_fw_rule(tunnel_port);
                // This ack also covers the firewall rule removal, so
                // `stop_and_wait` does not return before the rule is gone.
                let _ = ack_loop.send(());
                tracing::info!("WinDivert tunnel task exiting");
            });
        }

        // `stop()` shuts both directions down: `Recv` releases the intercept
        // thread parked in `recv`, and `Send` releases the inject thread if it
        // is parked in a full `WinDivertSend` queue. Both owner threads then
        // ack, so the session can be waited on and the handles closed
        // deterministically.
        let teardown = PlatformTeardown::new(
            {
                let wd_ic = Arc::clone(&wd_intercept);
                let wd_inj = Arc::clone(&wd_inject);
                let wd_sh = wd_shadow.clone();
                move || {
                    // The intercept handle is used for both recv and send
                    // (pass-through re-injection), so shut down both directions
                    // to release a thread parked in either call.
                    if let Err(e) = wd_ic.shutdown(WinDivertShutdownMode::Both) {
                        tracing::warn!("WinDivert intercept shutdown failed: {e}");
                    }
                    if let Err(e) = wd_inj.shutdown(WinDivertShutdownMode::Both) {
                        tracing::warn!("WinDivert inject shutdown failed: {e}");
                    }
                    if let Some(sh) = &wd_sh {
                        if let Err(e) = sh.shutdown(WinDivertShutdownMode::Both) {
                            tracing::warn!("WinDivert shadow shutdown failed: {e}");
                        }
                    }
                }
            },
            teardown_ack,
        );

        setup_guard.disarm();
        Ok(InterceptorHandle::new(
            shutdown_tx,
            counters,
            "WinDivert",
            Some(teardown),
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handle lifecycle helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Shuts the WinDivert handles down unless disarmed.
///
/// `start` opens the handles before it binds the tunnel socket and spawns the
/// async tunnel task. If any of that setup fails, `Drop` shuts the handles
/// down; the owner threads then exit and the last `Arc<OwnedHandle>` drop
/// closes the handle, so a failed start cannot leak WFP state. On success
/// `disarm` hands the handles to the running session.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
struct HandleShutdownGuard {
    intercept: Arc<OwnedHandle>,
    inject: Arc<OwnedHandle>,
    shadow: Option<Arc<OwnedHandle>>,
    armed: bool,
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl HandleShutdownGuard {
    fn new(
        intercept: Arc<OwnedHandle>,
        inject: Arc<OwnedHandle>,
        shadow: Option<Arc<OwnedHandle>>,
    ) -> Self {
        Self {
            intercept,
            inject,
            shadow,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl Drop for HandleShutdownGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self.intercept.shutdown(WinDivertShutdownMode::Both);
        let _ = self.inject.shutdown(WinDivertShutdownMode::Both);
        if let Some(shadow) = &self.shadow {
            let _ = shadow.shutdown(WinDivertShutdownMode::Both);
        }
    }
}

/// Log a WinDivert open failure, calling out stale WFP state when the OS
/// reports `FWP_E_IN_USE` so the fix (cold shutdown or killing the stale
/// process) is visible next to the raw error.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn log_open_failure(handle_role: &str, err: &std::io::Error) {
    if is_fwp_in_use(err) {
        tracing::error!(
            "❌ WinDivert {handle_role} open failed: {err}. A previous LightSpeed run \
             left its WFP filter/callout registered; do a full cold shutdown or close \
             the stale LightSpeed process, then try again."
        );
    } else {
        tracing::error!("❌ WinDivert {handle_role} open failed: {err}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Stub for non-Windows or missing feature
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
impl TrafficInterceptor for WinDivertInterceptor {
    fn platform_name(&self) -> &'static str {
        "WinDivert"
    }

    fn check_availability(&self) -> Result<(), String> {
        Err(
            "WinDivert requires Windows + windivert-redirect Cargo feature.\n\
             Build with: cargo build --features windivert-redirect"
                .to_string(),
        )
    }

    fn start(&self, _config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        anyhow::bail!(
            "WinDivert requires Windows + the 'windivert-redirect' feature.\n\
             Build with: cargo build --features windivert-redirect"
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Windows Firewall helpers
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn add_fw_rule(port: u16) {
    let exe = std::env::current_exe().unwrap_or_default();
    let name = format!("LightSpeed WinDivert Tunnel {}", port);
    match crate::process::silent_command("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={name}"),
            "protocol=UDP",
            "dir=in",
            "action=allow",
            &format!("program={}", exe.to_string_lossy()),
        ])
        .output()
    {
        Ok(o) if o.status.success() => {
            tracing::info!("🔓 Firewall: added inbound UDP rule for port {}", port);
        }
        Ok(o) => {
            tracing::warn!(
                "Firewall rule add may have failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            tracing::warn!(
                "Failed to run netsh for firewall (need Administrator?): {}",
                e
            );
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn remove_fw_rule(port: u16) {
    let name = format!("LightSpeed WinDivert Tunnel {}", port);
    match crate::process::silent_command("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={name}"),
        ])
        .output()
    {
        Ok(o) if o.status.success() => {
            tracing::info!("🔒 Firewall: removed WinDivert rule for port {}", port);
        }
        Ok(o) => {
            tracing::warn!(
                "Firewall rule remove may have failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("Failed to run netsh for firewall removal: {}", e);
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn add_fw_rule(_port: u16) {}
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn remove_fw_rule(_port: u16) {}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests: policy, exclusions and per-packet routing (no WinDivert needed)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::flows::{
        AttributionEvent, FlowMapConfig, FlowObservation, MAX_TUNNELED_FLOWS, WARN_TUNNELED_FLOWS,
    };
    use super::*;
    use std::time::Duration;

    /// The remote port every game-server destination in these tests uses.
    const SERVER_PORT: u16 = 7777;

    /// A public game server.
    fn public_server(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(104, 26, 1, last)
    }

    /// The game's local socket.
    fn game() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), 50_000)
    }

    fn dst_to(ip: Ipv4Addr) -> SocketAddrV4 {
        SocketAddrV4::new(ip, SERVER_PORT)
    }

    /// A SOCKET-layer connect observation: the at-attach attribution event.
    fn connect_obs(src: SocketAddrV4, remote: SocketAddrV4, pid: u32) -> FlowObservation {
        FlowObservation {
            key: FlowKey::new(src, remote, TransportProto::Udp),
            pid,
            event: AttributionEvent::SocketConnect,
        }
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    // ── exclusions ────────────────────────────────────────────────────────

    #[test]
    fn infra_denylist_constant_matches_the_design_and_is_enforced_in_userspace() {
        // The declared list, verbatim: DNS 53, DHCP 67/68, mDNS 5353, SSDP 1900,
        // NetBIOS 137-139 (design section 5.5).
        assert_eq!(
            INFRA_DENYLIST_PORTS,
            [53, 67, 68, 5353, 1900, 137, 138, 139]
        );

        for port in INFRA_DENYLIST_PORTS {
            let dst = SocketAddrV4::new(public_server(50), port);
            assert!(
                destination_is_excluded(dst, &[]),
                "port {port} must be excluded"
            );
        }
        // A normal public game destination is not excluded.
        assert!(!destination_is_excluded(dst_to(public_server(50)), &[]));
    }

    #[test]
    fn capture_filter_carries_the_relay_address_and_the_infra_denylist() {
        let relay = public_server(50);
        let filter = build_capture_filter(CapturePolicy::ProcessAll, &[relay]);

        assert!(filter.starts_with("udp and outbound"), "{filter}");
        assert!(
            filter.contains("ip.DstAddr != 104.26.1.50"),
            "the relay's own address must be excluded: {filter}"
        );
        for port in INFRA_DENYLIST_PORTS {
            assert!(
                filter.contains(&format!("udp.DstPort != {port}")),
                "port {port} must be excluded in the filter: {filter}"
            );
        }
        // The network layer cannot match a process, so the filter must not try.
        assert!(!filter.contains("rocessId"), "{filter}");
    }

    #[test]
    fn non_public_destination_never_enters_the_tunnel() {
        let t0 = Instant::now();
        let mut map = FlowMap::new(FlowMapConfig::default(), None);
        // Attribute the flow first, so only the exclusion can stop the tunnel.
        let remote = dst_to(Ipv4Addr::new(10, 0, 0, 7));
        assert!(map.attribute(connect_obs(game(), remote, 7), t0));

        for ip in [
            Ipv4Addr::new(10, 0, 0, 7),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(169, 254, 1, 1),
            Ipv4Addr::new(224, 0, 0, 1),
            Ipv4Addr::new(0, 0, 0, 0),
        ] {
            assert!(
                destination_is_excluded(dst_to(ip), &[]),
                "{ip} is not public and must be excluded"
            );
        }

        assert_eq!(
            route_outbound(&mut map, game(), remote, &[], t0, 100),
            OutboundRoute::Excluded
        );
        assert_eq!(
            map.tunneled_count(),
            0,
            "a non-public destination never takes a tunnel slot"
        );
    }

    #[test]
    fn relay_addresses_are_excluded_even_when_public() {
        let t0 = Instant::now();
        let relay = public_server(50);
        let mut map = FlowMap::new(FlowMapConfig::default(), None);
        assert!(map.attribute(connect_obs(game(), dst_to(relay), 7), t0));

        assert_eq!(
            route_outbound(&mut map, game(), dst_to(relay), &[relay], t0, 100),
            OutboundRoute::Excluded,
            "the client's own tunnel traffic must never be re-captured"
        );
        // Without the exclusion the same flow tunnels, so the test is not
        // vacuous: it is the relay rule that turned it direct.
        assert_eq!(
            route_outbound(&mut map, game(), dst_to(relay), &[], t0, 100),
            OutboundRoute::Tunnel
        );
    }

    // ── attribution and fail-open ─────────────────────────────────────────

    #[test]
    fn unattributed_flow_is_re_injected_never_dropped_or_relayed() {
        let t0 = Instant::now();
        // Process mode: the map knows PID 4242 and nothing else.
        let mut map = FlowMap::new(FlowMapConfig::default(), Some(4242));
        let remote = dst_to(public_server(51));

        assert_eq!(
            route_outbound(&mut map, game(), remote, &[], t0, 100),
            OutboundRoute::Reinject(DirectReason::Unattributed)
        );
        assert_eq!(map.tunneled_count(), 0);
        let report = map.report();
        assert_eq!(report.unattributed_flows, 1);
        assert_eq!(report.tracked, 0, "an unattributed flow is not tracked");
    }

    #[test]
    fn attributed_flow_is_tunneled_on_every_packet() {
        let t0 = Instant::now();
        let mut map = FlowMap::new(FlowMapConfig::default(), Some(4242));
        let remote = dst_to(public_server(52));
        assert!(map.attribute(connect_obs(game(), remote, 4242), t0));

        for i in 0..3 {
            assert_eq!(
                route_outbound(&mut map, game(), remote, &[], at(t0, i * 10), 100),
                OutboundRoute::Tunnel
            );
        }
    }

    // ── the cap and the warning, enforced where the map is used ───────────

    #[test]
    fn cap_admits_eight_tunneled_flows_and_the_ninth_goes_direct_unattributed() {
        let t0 = Instant::now();
        let mut map = FlowMap::new(FlowMapConfig::default(), None);
        let src = game();

        for i in 0..MAX_TUNNELED_FLOWS as u8 {
            let remote = dst_to(public_server(i + 1));
            assert!(map.attribute(connect_obs(src, remote, 7), t0));
            assert_eq!(
                route_outbound(&mut map, src, remote, &[], t0, 100),
                OutboundRoute::Tunnel
            );
        }
        assert_eq!(map.tunneled_count(), MAX_TUNNELED_FLOWS);

        let ninth = dst_to(Ipv4Addr::new(104, 26, 2, 1));
        assert!(map.attribute(connect_obs(src, ninth, 7), t0));
        assert_eq!(
            route_outbound(&mut map, src, ninth, &[], t0, 100),
            OutboundRoute::Reinject(DirectReason::Unattributed),
            "the ninth flow fails open and is reported UNATTRIBUTED"
        );
        let report = map.report();
        assert_eq!(report.tunneled, MAX_TUNNELED_FLOWS);
        assert_eq!(report.over_cap_packets, 1);
        assert_eq!(report.unattributed_flows, 1);
    }

    #[test]
    fn warning_fires_at_six_tunneled_flows() {
        let t0 = Instant::now();
        let mut map = FlowMap::new(FlowMapConfig::default(), None);
        let src = game();

        for i in 0..(WARN_TUNNELED_FLOWS as u8 - 1) {
            let remote = dst_to(public_server(i + 1));
            assert!(map.attribute(connect_obs(src, remote, 7), t0));
            assert_eq!(
                route_outbound(&mut map, src, remote, &[], t0, 100),
                OutboundRoute::Tunnel
            );
        }
        assert_eq!(map.tunneled_count(), WARN_TUNNELED_FLOWS - 1);
        assert!(
            map.report().warning.is_none(),
            "no warning below the threshold"
        );

        let sixth = dst_to(public_server(60));
        assert!(map.attribute(connect_obs(src, sixth, 7), t0));
        assert_eq!(
            route_outbound(&mut map, src, sixth, &[], t0, 100),
            OutboundRoute::Tunnel
        );

        let warning = map.report().warning.expect("warning at the threshold");
        assert_eq!(warning.tunneled, WARN_TUNNELED_FLOWS);
        assert_eq!(warning.cap, MAX_TUNNELED_FLOWS);
        assert!(warning.message.contains("3600"), "{}", warning.message);
    }

    // ── the policy variants ───────────────────────────────────────────────

    #[test]
    fn policy_variants_shape_the_capture_filter() {
        // ProcessAll: every outbound UDP datagram, no port scope at all.
        let all = build_capture_filter(CapturePolicy::ProcessAll, &[]);
        assert_eq!(
            all,
            "udp and outbound and udp.DstPort != 53 and udp.DstPort != 67 and udp.DstPort != 68 \
             and udp.DstPort != 5353 and udp.DstPort != 1900 and udp.DstPort != 137 \
             and udp.DstPort != 138 and udp.DstPort != 139"
        );
        assert!(!all.contains("udp.DstPort >="), "{all}");

        // PortRange: the built-in game path, unchanged.
        let ranged = build_capture_filter(
            CapturePolicy::PortRange {
                lo: 28015,
                hi: 28017,
            },
            &[],
        );
        assert!(
            ranged
                .starts_with("udp and outbound and udp.DstPort >= 28015 and udp.DstPort <= 28017"),
            "{ranged}"
        );
        let single = build_capture_filter(CapturePolicy::PortRange { lo: 7777, hi: 7777 }, &[]);
        assert!(
            single.starts_with("udp and outbound and udp.DstPort == 7777"),
            "{single}"
        );

        // The shadow mirror swaps the direction and keeps the relay exclusion.
        let shadow = build_shadow_filter(
            CapturePolicy::PortRange { lo: 7777, hi: 7777 },
            &[public_server(50)],
        );
        assert_eq!(
            shadow,
            "udp and inbound and udp.SrcPort == 7777 and ip.SrcAddr != 104.26.1.50"
        );
    }
}
