//! Background keepalive + redirect engine - drives the tunnel loop from a GUI context.
//!
//! [`LightSpeedEngine`] can be created on any thread.  It spawns async tasks
//! onto the provided Tokio [`Handle`] and updates a shared [`EngineStatus`]
//! that the GUI polls on every frame.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::games::TransportProto;
use crate::interceptor::process_scanner::find_pid_by_name;
use crate::interceptor::InterceptorConfig;
use crate::modes::capture_mode::CaptureStatSlot;
use crate::modes::redirect_windivert::WinDivertStatSlot;
use crate::redirect::{RedirectStats, UdpRedirect};

// ── Public types ─────────────────────────────────────────────────────────

/// Snapshot of tunnel state - cheap to clone, safe to share.
#[derive(Clone, Debug, Default)]
pub struct EngineStatus {
    /// Whether the keepalive loop is actively running.
    pub connected: bool,
    /// Proxy address as a display string, e.g. `"YOUR_PROXY_IP:4434"`.
    pub proxy_addr: String,
    /// Latest round-trip time in milliseconds (0.0 when no measurement).
    pub latest_rtt_ms: f64,
    /// Rolling window of up to 120 RTT measurements (ms), newest last.
    pub rtt_history: Vec<f64>,
    /// Total keepalive packets sent this session.
    pub packets_sent: u64,
    /// Total keepalive echo packets received this session.
    pub packets_received: u64,
    /// Generation counter - incremented each time `connect()` is called.
    /// Used to prevent old tasks from overwriting status after reconnect.
    pub keepalive_generation: u64,

    // ── Control-plane registration ────────────────────────────────
    /// Whether the relay's QUIC control plane accepted a session registration.
    pub control_registered: bool,
    /// Registry node id of the relay the control plane registered with
    /// (`relay-lax-1`, …). Set by the GUI; `None` when unknown.
    pub node_id: Option<String>,
    /// Session token returned by the relay's control plane.
    pub session_token: Option<u32>,
    /// User-facing reason registration has not succeeded (`None` while
    /// pending or registered).
    pub registration_error: Option<String>,

    // ── Redirect / game routing ───────────────────────────────────
    /// Whether a game redirect is actively running.
    pub redirect_active: bool,
    /// Name of the game being optimized (e.g. "Rust").
    pub redirect_game: String,
    /// Game server address string (e.g. "1.2.3.4:28015").
    pub redirect_server: String,
    /// Local port the game should connect to.
    pub redirect_local_port: u16,
    /// Whether FEC is enabled for the redirect.
    pub redirect_fec: bool,
    /// Packets forwarded Game → Proxy.
    pub redirect_pkts_out: u64,
    /// Packets forwarded Proxy → Game.
    pub redirect_pkts_in: u64,
    /// Error count in the redirect loop.
    pub redirect_errors: u64,
    /// FEC parity packets sent.
    pub redirect_fec_parity: u64,
    /// FEC packets recovered.
    pub redirect_fec_recovered: u64,
    /// Error description if redirect failed to start.
    pub redirect_error: Option<String>,

    // ── Capture / pcap mode ───────────────────────────────────────
    /// Whether pcap capture mode is actively running.
    pub capture_active: bool,
    /// Name of the game being captured (e.g. "Rust").
    pub capture_game: String,
    /// Human-readable NIC description being captured on.
    pub capture_interface: String,
    /// Whether FEC is enabled for capture.
    pub capture_fec: bool,
    /// Packets captured Game → Proxy.
    pub capture_pkts_out: u64,
    /// Packets received from Proxy (before injection).
    pub capture_pkts_in: u64,
    /// Packets successfully injected back to game.
    pub capture_injected: u64,
    /// Inject errors on inbound path.
    pub capture_errors: u64,
    /// FEC packets recovered on inbound path.
    pub capture_fec_recovered: u64,
    /// Error description if capture failed to start.
    pub capture_error: Option<String>,
    /// Active BPF filter string (set at capture start for diagnostics).
    pub capture_bpf: String,

    // ── WinDivert active redirect mode ────────────────────────────
    /// Whether WinDivert active redirect is running.
    pub windivert_active: bool,
    /// Game server being redirected.
    pub windivert_server: String,
    /// Packets intercepted by WinDivert (Game → Proxy).
    pub windivert_intercepted: u64,
    /// Proxy responses received.
    pub windivert_from_proxy: u64,
    /// Spoofed packets injected back to game.
    pub windivert_injected: u64,
    /// Errors in WinDivert intercept/inject.
    pub windivert_errors: u64,
    /// Error description if WinDivert failed to start.
    pub windivert_error: Option<String>,

    // ── OOP TrafficInterceptor (multiplatform MITM) ───────────────────────
    /// Whether the OOP interceptor is active.
    pub interceptor_active: bool,
    /// Platform name ("WinDivert", "nftables/iptables", "pfctl", …).
    pub interceptor_platform: &'static str,
    /// Game server being intercepted.
    pub interceptor_server: String,
    /// Packets intercepted outbound (Game → Proxy).
    pub interceptor_intercepted: u64,
    /// Proxy responses received.
    pub interceptor_from_proxy: u64,
    /// Packets injected inbound (Proxy → Game).
    pub interceptor_injected: u64,
    /// Errors.
    pub interceptor_errors: u64,
    /// Every packet the capture handle produced, before any policy, so a UI can
    /// tell "nothing needed relaying" from "the filter matches nothing".
    pub interceptor_seen: u64,
    /// Outbound datagrams the tunnel actually handed to the OS toward the relay,
    /// against `interceptor_intercepted`, which counts what was decided to be
    /// relayed. The pair tells a held flow (both zero) from a relay that is not
    /// taking the traffic (intercepted climbing, sent not).
    pub interceptor_tunnel_sent: u64,
    /// Plain-language explanation when the capture is producing nothing, so the
    /// app can admit it is not working instead of looking healthy.
    pub capture_diagnosis: Option<String>,
    /// Whether the run is permitted to tunnel, or `None` when the backend has no
    /// gate. `Some(false)` means traffic is deliberately held direct until a
    /// relay has been chosen for the known game server.
    pub tunnelling_enabled: Option<bool>,
    /// Smoothed `direct - relayed` advantage in milliseconds, when measured:
    /// positive means the relay is actually beating the direct path.
    pub bypass_advantage_ms: Option<f32>,
    /// Payloads forwarded with fragmentation allowed for exceeding the
    /// conservative tunnel payload budget.
    pub interceptor_payloads_over_budget: u64,
    /// Error description if the interceptor failed to start.
    pub interceptor_error: Option<String>,

    // ── Bypass gate ───────────────────────────────────────────────────────
    /// Active bypass mode ("dry_run", "auto", "never", "always").
    pub bypass_mode: String,
    /// Bypasses the gate decided and the action layer honoured.
    pub bypass_allowed: u64,
    /// Bypasses computed but not applied, so the relay was kept.
    pub bypass_refused: u64,
    /// Bypass-gate evaluations that produced a decision.
    pub bypass_decisions: u64,
    /// Relay<->Direct transitions recorded by the gate.
    pub bypass_flips: u64,
    /// Pre-gate evaluations that received a fresh client->relay RTT.
    pub bypass_pre_gate_rtt: u64,
    /// Every steady-state bypass evaluation that ran.
    pub bypass_evaluations: u64,
    /// Evaluations whose computed decision was Relay.
    pub bypass_relay: u64,
    /// Evaluations whose computed decision was Direct.
    pub bypass_direct: u64,
    /// Evaluations whose computed decision was Hold.
    pub bypass_hold: u64,
}

// ── Engine ───────────────────────────────────────────────────────────────

type Shared = Arc<RwLock<EngineStatus>>;

/// Lock `lock` for reading, recovering a poisoned lock instead of panicking.
///
/// Poisoning is permanent: one panic while a write guard is held would
/// otherwise make every later `read` panic too - and the GUI takes
/// [`LightSpeedEngine::snapshot`] once per frame, so a single background panic
/// would crash the whole window rather than just the task that panicked.
pub(crate) fn read_or_recover<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// [`read_or_recover`] for write guards.
pub(crate) fn write_or_recover<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Control-plane port used for relay registration when no override is set.
///
/// Matches the client config default (`proxy.quic_port`); self-hosted relays on
/// another port call [`LightSpeedEngine::set_control_port`].
pub const DEFAULT_CONTROL_PORT: u16 = 4433;

/// Manages the keepalive loop, optional game redirect, and optional pcap capture.
/// Designed for use from GUI code running outside a Tokio context.
pub struct LightSpeedEngine {
    rt: Handle,
    status: Shared,
    control_port: u16,
    supervised_paths: Vec<SocketAddrV4>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_stats: Option<Arc<RedirectStats>>,
    /// Shutdown sender for active capture task.
    capture_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Slot filled by the capture task with live stat Arc handles.
    capture_stat_slot: Option<CaptureStatSlot>,
    /// Shutdown sender for active WinDivert redirect task.
    windivert_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Completion acknowledgement from the active WinDivert redirect task.
    ///
    /// The task sends on this channel once its `RedirectTeardown` has closed
    /// both WinDivert handles, so [`Self::stop_windivert`] can wait a bounded
    /// time before the GUI's `Quit` (`std::process::exit`) tears the process
    /// down. `None` when no redirect task was spawned (for example on
    /// non-Windows builds).
    windivert_done_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// Slot filled by the WinDivert task with live stat Arc handles.
    windivert_stat_slot: Option<WinDivertStatSlot>,
    /// Live handle for the OOP TrafficInterceptor (multiplatform MITM).
    interceptor_handle: Option<crate::interceptor::InterceptorHandle>,
    /// Watcher that ends the current hold once its predicate says so.
    ///
    /// See [`Self::release_hold_when_connected`]; `None` when the run is not
    /// held or the backend has no gate.
    hold_watch: Option<tokio::task::JoinHandle<()>>,
    /// Periodic telemetry flush task, running while a relay is connected and
    /// telemetry is enabled.
    telemetry_flush: Option<tokio::task::JoinHandle<()>>,
    /// Relay the telemetry flusher reports to, refreshed on each connect/start.
    telemetry_proxy: Option<SocketAddrV4>,
    /// Wire game id attached to telemetry reports (`UNKNOWN` until a game runs).
    telemetry_game_id: u8,
}

impl LightSpeedEngine {
    /// Create a new (disconnected) engine backed by `rt`.
    pub fn new(rt: Handle) -> Self {
        // The engine can start an interceptor, redirect, or capture, all of
        // which need local measurement. Install it independently of telemetry
        // reporting; nothing is reported unless telemetry is enabled.
        crate::latency::install_measurement();
        Self {
            rt,
            status: Arc::new(RwLock::new(EngineStatus::default())),
            control_port: DEFAULT_CONTROL_PORT,
            supervised_paths: Vec::new(),
            shutdown_tx: None,
            redirect_shutdown_tx: None,
            redirect_stats: None,
            capture_shutdown_tx: None,
            capture_stat_slot: None,
            windivert_shutdown_tx: None,
            windivert_done_rx: None,
            windivert_stat_slot: None,
            interceptor_handle: None,
            hold_watch: None,
            telemetry_flush: None,
            telemetry_proxy: None,
            telemetry_game_id: lightspeed_protocol::game_id::UNKNOWN,
        }
    }

    /// Override the QUIC control-plane port used for relay registration.
    ///
    /// Defaults to [`DEFAULT_CONTROL_PORT`]; call this before [`Self::connect`]
    /// (or any `start_*` mode) to reach a self-hosted relay on another port.
    pub fn set_control_port(&mut self, port: u16) {
        self.control_port = port;
    }

    /// The control-plane port used for relay registration.
    pub fn control_port(&self) -> u16 {
        self.control_port
    }

    /// Start (or restart) the keepalive loop toward `proxy_addr`.
    pub fn connect(&mut self, proxy_addr: SocketAddrV4) {
        self.disconnect();
        let (tx, rx) = oneshot::channel();
        self.shutdown_tx = Some(tx);
        let generation = {
            let mut s = write_or_recover(&self.status);
            s.keepalive_generation = s.keepalive_generation.wrapping_add(1);
            s.connected = true;
            s.proxy_addr = proxy_addr.to_string();
            s.packets_sent = 0;
            s.packets_received = 0;
            s.rtt_history.clear();
            s.latest_rtt_ms = 0.0;
            s.control_registered = false;
            s.node_id = None;
            s.session_token = None;
            s.registration_error = None;
            s.keepalive_generation
        };
        let status = Arc::clone(&self.status);
        self.rt
            .spawn(run_keepalive(proxy_addr, status, rx, generation));
        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
    }

    /// Record the registry node id of the relay currently selected by the GUI.
    pub fn set_node_id(&mut self, node_id: Option<String>) {
        if let Ok(mut s) = self.status.write() {
            s.node_id = node_id;
        }
    }

    /// Enable or disable anonymous telemetry for this engine.
    ///
    /// Enabling installs the process-wide collector and latency tracker
    /// (idempotent) and starts periodic flushing for the current relay.
    /// Disabling stops flushing and gates off further recording. The
    /// process-wide installation is never removed; it is only muted.
    pub fn set_telemetry_enabled(&mut self, enabled: bool) {
        if enabled {
            let _ = crate::telemetry::install();
            crate::telemetry::set_enabled(true);
            self.refresh_telemetry_flush();
        } else {
            crate::telemetry::set_enabled(false);
            self.stop_telemetry_flush();
        }
    }

    /// Set the wire game id attached to telemetry reports (`0` = unknown).
    pub fn set_telemetry_game(&mut self, game_key: &str) {
        self.telemetry_game_id = lightspeed_protocol::game_id::id_for_key(game_key);
    }

    /// Remember the relay for telemetry and (re)start flushing when enabled.
    fn note_telemetry_proxy(&mut self, proxy_addr: SocketAddrV4) {
        self.telemetry_proxy = Some(proxy_addr);
        self.refresh_telemetry_flush();
    }

    /// (Re)start the periodic flush toward the remembered relay.
    fn refresh_telemetry_flush(&mut self) {
        self.stop_telemetry_flush();
        if !crate::telemetry::is_enabled() {
            return;
        }
        let Some(proxy) = self.telemetry_proxy else {
            return;
        };
        let Some(collector) = crate::telemetry::paths::global_collector().cloned() else {
            return;
        };
        let ctx = crate::telemetry::context::TelemetryContext {
            game_id: self.telemetry_game_id,
            country: crate::telemetry::context::detect_country(),
        };
        let host = proxy.ip().to_string();
        // `spawn_periodic_flush` calls `tokio::spawn`, which needs a runtime
        // context; the GUI calls engine methods from the egui thread.
        let _guard = self.rt.enter();
        self.telemetry_flush = Some(crate::telemetry::spawn_periodic_flush(
            collector,
            host,
            Some(proxy),
            ctx,
        ));
        tracing::info!("Telemetry flush started toward {}", proxy.ip());
    }

    /// Stop the periodic flush (if running).
    fn stop_telemetry_flush(&mut self) {
        if let Some(handle) = self.telemetry_flush.take() {
            handle.abort();
        }
    }

    /// Spawn the QUIC control-plane registration task, recording its outcome
    /// in the shared status instead of discarding it. The relay is tracked so
    /// [`Self::disconnect`] and mode switches can stop its reconnect supervisor.
    fn spawn_registration(&mut self, proxy_addr: SocketAddrV4) {
        if !self.supervised_paths.contains(&proxy_addr) {
            self.supervised_paths.push(proxy_addr);
        }
        let status = Arc::clone(&self.status);
        let control_port = self.control_port;
        self.rt.spawn(async move {
            let result = crate::quic::register_session(proxy_addr, control_port).await;
            if let Ok(mut s) = status.write() {
                apply_registration_result(&mut s, result);
            }
        });
    }

    /// Stop every supervised relay registration this engine started, leaving
    /// the data-plane tokens intact. Tokens survive a transient disconnect or
    /// mode switch; only process shutdown calls `session::reset_all_tokens`.
    fn stop_supervised(&mut self) {
        for addr in self.supervised_paths.drain(..) {
            crate::quic::stop_supervisor(addr);
        }
    }

    /// Stop the keepalive loop and every supervised relay registration.
    pub fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.stop_supervised();
        self.stop_telemetry_flush();
        if let Ok(mut s) = self.status.write() {
            s.connected = false;
        }
    }

    /// Start a game redirect: forwards game UDP through the proxy.
    ///
    /// The game should connect to `127.0.0.1:<local_port>`. Stops any
    /// previously running redirect first.
    pub fn start_redirect(
        &mut self,
        game_server: SocketAddrV4,
        local_port: u16,
        fec: bool,
        fec_k: u8,
        game_name: String,
        proxy_addr: SocketAddrV4,
    ) {
        // Tear down any previous redirect first.
        self.stop_redirect();

        let mut redirect = UdpRedirect::new(local_port, game_server, proxy_addr);
        if fec {
            redirect = redirect.with_fec(fec_k);
        }
        let stats = Arc::clone(&redirect.stats);
        self.redirect_stats = Some(Arc::clone(&stats));

        let (tx, rx) = oneshot::channel();
        self.redirect_shutdown_tx = Some(tx);

        {
            let mut s = write_or_recover(&self.status);
            s.redirect_active = true;
            s.redirect_game = game_name;
            s.redirect_server = game_server.to_string();
            s.redirect_local_port = local_port;
            s.redirect_fec = fec;
            s.redirect_pkts_out = 0;
            s.redirect_pkts_in = 0;
            s.redirect_errors = 0;
            s.redirect_fec_parity = 0;
            s.redirect_fec_recovered = 0;
            s.redirect_error = None;
            s.control_registered = false;
            s.session_token = None;
            s.registration_error = None;
        }

        let status = Arc::clone(&self.status);
        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
        self.rt.spawn(async move {
            // run_with_shutdown takes &self and is Send, but UdpRedirect isn't
            // Arc'd - we move it into the task.
            match redirect.run_with_shutdown(rx).await {
                Ok(()) => {
                    tracing::info!("Redirect stopped cleanly");
                }
                Err(e) => {
                    tracing::error!("Redirect error: {}", e);
                    if let Ok(mut s) = status.write() {
                        s.redirect_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.write() {
                s.redirect_active = false;
            }
        });
    }

    /// Stop the game redirect (if running).
    pub fn stop_redirect(&mut self) {
        if let Some(tx) = self.redirect_shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.stop_supervised();
        self.redirect_stats = None;
        if let Ok(mut s) = self.status.write() {
            s.redirect_active = false;
        }
    }

    // ── Capture (pcap) mode ───────────────────────────────────────────────

    /// Start transparent pcap capture mode for the given game key.
    ///
    /// Sniffs game UDP directly from the NIC, tunnels through `proxy_addr`,
    /// and injects responses back - no server IP or game console command needed.
    /// Requires Npcap installed and the process running as Administrator.
    ///
    /// Returns `Err(msg)` if the game key is unknown; start errors (e.g., not
    /// admin) surface asynchronously via `snapshot().capture_error`.
    pub fn start_capture(
        &mut self,
        game_key: &str,
        proxy_addr: SocketAddrV4,
        interface_opt: Option<String>,
        fec: bool,
        fec_k: u8,
    ) -> Result<(), String> {
        self.stop_capture();

        // Resolve game config from key string (built-ins first, then user profiles).
        let game_box = crate::games::resolve(game_key).map_err(|e| e.to_string())?;

        // Build the interface display string for the status panel.
        let iface_desc = interface_opt
            .as_deref()
            .map(crate::capture::interface_description)
            .unwrap_or_else(|| {
                crate::capture::pick_best_interface()
                    .map(|n| crate::capture::interface_description(&n))
                    .unwrap_or_else(|| "auto-detect".to_string())
            });

        // Build the BPF filter now (runs tasklist/netstat synchronously on
        // the calling thread) so the GUI shows it immediately AND the capture
        // task reuses it without re-running netstat at a different moment.
        let preview_filter = game_box.build_capture_filter();
        let preview_bpf = preview_filter.bpf.clone();
        tracing::info!("🔍 Capture BPF: {}", preview_bpf);

        // Resolve the best capture interface now (before spawning) so the
        // pcap backend uses our scored pick instead of "first device in list".
        let resolved_interface = interface_opt.or_else(crate::capture::pick_best_interface);
        tracing::info!(
            "🖧 Capture interface: {}",
            resolved_interface.as_deref().unwrap_or("(none found)")
        );

        // Create the stat slot - the capture task will fill it once running.
        let stat_slot: CaptureStatSlot = Arc::new(Mutex::new(None));
        self.capture_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.capture_shutdown_tx = Some(tx);

        {
            let mut s = write_or_recover(&self.status);
            s.capture_active = true;
            s.capture_game = game_box.name().to_string();
            s.capture_interface = iface_desc;
            s.capture_fec = fec;
            s.capture_pkts_out = 0;
            s.capture_pkts_in = 0;
            s.capture_injected = 0;
            s.capture_errors = 0;
            s.capture_fec_recovered = 0;
            s.capture_error = None;
            s.capture_bpf = preview_bpf;
            s.control_registered = false;
            s.session_token = None;
            s.registration_error = None;
        }

        let status = Arc::clone(&self.status);
        let proxy_id = proxy_addr.to_string();

        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
        self.rt.spawn(async move {
            // game_box owned here; reference valid for the duration of the .await
            let game: Box<dyn crate::games::GameConfig> = game_box;
            let online_learner = Arc::new(tokio::sync::Mutex::new(
                crate::ml::online::OnlineLearner::new(),
            ));
            let keepalive_timestamps =
                Arc::new(tokio::sync::Mutex::new(HashMap::<u16, Instant>::new()));

            match crate::modes::capture_mode::run_capture_mode_with_shutdown(
                game.as_ref(),
                proxy_addr,
                proxy_id,
                "unknown".to_string(),
                online_learner,
                keepalive_timestamps,
                fec,
                fec_k,
                resolved_interface, // use the scored interface pick
                rx,
                Some(stat_slot),
            )
            .await
            {
                Ok(()) => tracing::info!("Capture stopped cleanly"),
                Err(e) => {
                    tracing::error!("Capture error: {}", e);
                    if let Ok(mut s) = status.write() {
                        s.capture_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.write() {
                s.capture_active = false;
            }
        });

        Ok(())
    }

    /// Stop the pcap capture task (if running).
    pub fn stop_capture(&mut self) {
        if let Some(tx) = self.capture_shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.stop_supervised();
        self.capture_stat_slot = None;
        if let Ok(mut s) = self.status.write() {
            s.capture_active = false;
        }
    }

    /// Returns the best interface name for capture (prefers wired Ethernet).
    pub fn auto_pick_capture_interface() -> Option<String> {
        crate::capture::pick_best_interface()
    }

    /// Returns a human-readable description for a capture interface.
    pub fn capture_interface_description(name: &str) -> String {
        crate::capture::interface_description(name)
    }

    // ── WinDivert active redirect mode ────────────────────────────────────

    /// Start WinDivert kernel-level packet interception.
    ///
    /// Intercepts outbound game UDP to `server_addr`, tunnels through
    /// `proxy_addr`, and injects spoofed responses back - severs the game's
    /// direct path so in-game ping reflects the proxy RTT.
    /// Requires WinDivert64.sys + WinDivert.dll next to the exe and
    /// Administrator privileges.
    ///
    /// Gated on `#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]`
    /// at the implementation layer; returns `Err` on non-Windows or when the
    /// feature is disabled.
    #[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
    pub fn start_windivert(
        &mut self,
        server_addr: SocketAddrV4,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
    ) -> Result<(), String> {
        self.start_windivert_with_transport(
            server_addr,
            proxy_addr,
            fec,
            fec_k,
            TransportProto::Udp,
        )
    }

    /// [`Self::start_windivert`] with an explicit game transport, so a TCP
    /// profile (Minecraft Java Edition) can enable the terminator path while
    /// every existing call site keeps the UDP default.
    #[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
    pub fn start_windivert_with_transport(
        &mut self,
        server_addr: SocketAddrV4,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        transport: TransportProto,
    ) -> Result<(), String> {
        self.stop_windivert();

        use crate::capture::windivert_redirect::WinDivertConfig;
        use crate::modes::redirect_windivert::run_windivert_mode_with_shutdown;

        let cfg = WinDivertConfig {
            server_addr: Some(server_addr),
            port_range: (server_addr.port(), server_addr.port()),
            proxy_addr,
            fec_enabled: fec,
            fec_k,
            transport,
        };

        let stat_slot: WinDivertStatSlot = Arc::new(Mutex::new(None));
        self.windivert_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.windivert_shutdown_tx = Some(tx);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        self.windivert_done_rx = Some(done_rx);

        {
            let mut s = write_or_recover(&self.status);
            s.windivert_active = true;
            s.windivert_server = server_addr.to_string();
            s.windivert_intercepted = 0;
            s.windivert_from_proxy = 0;
            s.windivert_injected = 0;
            s.windivert_errors = 0;
            s.windivert_error = None;
            s.control_registered = false;
            s.session_token = None;
            s.registration_error = None;
        }

        let status = Arc::clone(&self.status);
        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
        self.rt.spawn(async move {
            match run_windivert_mode_with_shutdown(cfg, rx, Some(stat_slot)).await {
                Ok(()) => tracing::info!("WinDivert redirect stopped cleanly"),
                Err(e) => {
                    tracing::error!("WinDivert redirect error: {}", e);
                    if let Ok(mut s) = status.write() {
                        s.windivert_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.write() {
                s.windivert_active = false;
            }
            // Reached only after `run_windivert_mode_with_shutdown` returned,
            // which means `RedirectTeardown` already closed both handles.
            let _ = done_tx.send(());
        });

        Ok(())
    }

    /// Stub for non-Windows / feature-disabled builds - always returns `Err`.
    #[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
    pub fn start_windivert(
        &mut self,
        _server_addr: SocketAddrV4,
        _proxy_addr: SocketAddrV4,
        _fec: bool,
        _fec_k: u8,
    ) -> Result<(), String> {
        Err("WinDivert redirect requires Windows + windivert-redirect feature".to_string())
    }

    /// Stub for non-Windows / feature-disabled builds.
    #[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
    pub fn start_windivert_with_transport(
        &mut self,
        _server_addr: SocketAddrV4,
        _proxy_addr: SocketAddrV4,
        _fec: bool,
        _fec_k: u8,
        _transport: TransportProto,
    ) -> Result<(), String> {
        Err("WinDivert redirect requires Windows + windivert-redirect feature".to_string())
    }

    /// Start WinDivert kernel-level interception using a game's port range.
    ///
    /// No server IP is required - the first outbound Game UDP packet whose
    /// destination port falls in `[port_lo, port_hi]` is treated as the game
    /// server.  All subsequent game packets are tunnelled through `proxy_addr`.
    /// Non-game traffic that accidentally passes the broad filter is re-injected
    /// untouched.
    ///
    /// This is the "just click Start, then launch your game"
    /// experience.
    #[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
    pub fn start_windivert_for_game(
        &mut self,
        port_lo: u16,
        port_hi: u16,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
    ) -> Result<(), String> {
        self.start_windivert_for_game_with_transport(
            port_lo,
            port_hi,
            proxy_addr,
            fec,
            fec_k,
            TransportProto::Udp,
        )
    }

    /// [`Self::start_windivert_for_game`] with an explicit game transport, so
    /// a TCP profile (Minecraft Java Edition) can enable the terminator path.
    #[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
    pub fn start_windivert_for_game_with_transport(
        &mut self,
        port_lo: u16,
        port_hi: u16,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        transport: TransportProto,
    ) -> Result<(), String> {
        self.stop_windivert();

        use crate::capture::windivert_redirect::WinDivertConfig;
        use crate::modes::redirect_windivert::run_windivert_mode_with_shutdown;

        let cfg = WinDivertConfig {
            server_addr: None, // auto-detect from first packet
            port_range: (port_lo, port_hi),
            proxy_addr,
            fec_enabled: fec,
            fec_k,
            transport,
        };

        let stat_slot: WinDivertStatSlot = Arc::new(Mutex::new(None));
        self.windivert_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.windivert_shutdown_tx = Some(tx);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        self.windivert_done_rx = Some(done_rx);

        {
            let mut s = write_or_recover(&self.status);
            s.windivert_active = true;
            s.windivert_server = format!("Auto-detecting (ports {}-{})…", port_lo, port_hi);
            s.windivert_intercepted = 0;
            s.windivert_from_proxy = 0;
            s.windivert_injected = 0;
            s.windivert_errors = 0;
            s.windivert_error = None;
            s.control_registered = false;
            s.session_token = None;
            s.registration_error = None;
        }

        let status = Arc::clone(&self.status);
        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
        self.rt.spawn(async move {
            match run_windivert_mode_with_shutdown(cfg, rx, Some(stat_slot)).await {
                Ok(()) => tracing::info!("WinDivert auto-redirect stopped cleanly"),
                Err(e) => {
                    tracing::error!("WinDivert auto-redirect error: {}", e);
                    if let Ok(mut s) = status.write() {
                        s.windivert_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.write() {
                s.windivert_active = false;
            }
            // Reached only after `run_windivert_mode_with_shutdown` returned,
            // which means `RedirectTeardown` already closed both handles.
            let _ = done_tx.send(());
        });

        Ok(())
    }

    /// Stub for non-Windows / feature-disabled builds.
    #[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
    pub fn start_windivert_for_game(
        &mut self,
        _port_lo: u16,
        _port_hi: u16,
        _proxy_addr: SocketAddrV4,
        _fec: bool,
        _fec_k: u8,
    ) -> Result<(), String> {
        Err("WinDivert redirect requires Windows + windivert-redirect feature".to_string())
    }

    /// Stub for non-Windows / feature-disabled builds.
    #[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
    pub fn start_windivert_for_game_with_transport(
        &mut self,
        _port_lo: u16,
        _port_hi: u16,
        _proxy_addr: SocketAddrV4,
        _fec: bool,
        _fec_k: u8,
        _transport: TransportProto,
    ) -> Result<(), String> {
        Err("WinDivert redirect requires Windows + windivert-redirect feature".to_string())
    }

    /// Stop the WinDivert redirect task (if running) and wait a bounded time
    /// for it to confirm that both WinDivert handles were closed.
    pub fn stop_windivert(&mut self) {
        if let Some(tx) = self.windivert_shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.stop_supervised();
        let done =
            wait_for_redirect_ack(self.windivert_done_rx.take(), Duration::from_millis(1500));
        if !done {
            tracing::warn!(
                "WinDivert redirect did not acknowledge shutdown within 1500ms; \
                 handles may still be closing"
            );
        }
        self.windivert_stat_slot = None;
        if let Ok(mut s) = self.status.write() {
            s.windivert_active = false;
        }
    }

    /// How often [`Self::release_hold_when_connected`] asks whether the relay it
    /// is waiting for has connected.
    ///
    /// Lives here rather than on the GUI's frame loop because that loop stops
    /// when the window is not being redrawn, which is exactly when a player is in
    /// a match and the hold must end without the GUI's help.
    pub const HOLD_CONNECT_POLL: std::time::Duration = std::time::Duration::from_millis(100);

    // ── OOP TrafficInterceptor (multiplatform MITM) ───────────────────────

    /// Start the OOP `TrafficInterceptor` for a game identified by `game_key`,
    /// with no process target - the built-in ports path.
    ///
    /// Thin wrapper over
    /// [`start_interceptor_with_target`](Self::start_interceptor_with_target)
    /// passing `None`, so every existing caller keeps the ports-path behaviour.
    pub fn start_interceptor(
        &mut self,
        game_key: &str,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        port_range_override: Option<(u16, u16)>,
    ) -> Result<(), String> {
        self.start_interceptor_with_target(
            game_key,
            proxy_addr,
            fec,
            fec_k,
            port_range_override,
            None,
        )
    }

    /// Start the OOP `TrafficInterceptor`, optionally scoped to a picked process.
    ///
    /// This is the **recommended** entry point for all new integrations:
    ///
    /// 1. Resolves the game profile.
    /// 2. Creates the best available interceptor for the current OS.
    /// 3. Starts kernel-level MITM interception automatically.
    ///
    /// **Precedence, authoritative.** `process_target` wins over
    /// `port_range_override`: when a process is picked the config is built for
    /// that process, and no port range decides the target. `None` is the
    /// built-in ports path, where `port_range_override` (the Manual / Custom
    /// range the GUI parsed, when present) beats the profile's own range, and
    /// `None` keeps the profile default.
    ///
    /// A process target is bound against the live process table **here**, at
    /// start time: the image name is the identity, so the PID it binds to is
    /// the one running now and a relaunch under a new PID still matches. When
    /// the named process is not running the call returns `Err` naming it - it
    /// never silently degrades to a port-range filter.
    ///
    /// The platform then applies the target as its capabilities allow: Windows
    /// drives the WinDivert network layer, whose filter cannot match
    /// `processId` (that needs the Flow/Socket layer), so it stays
    /// address/port based and the target narrows it through the scanner's
    /// routes.
    ///
    /// Returns `Err(msg)` synchronously if the game key is unknown, the picked
    /// process is not running, the interceptor is unavailable, or the start
    /// call fails.
    pub fn start_interceptor_with_target(
        &mut self,
        game_key: &str,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        port_range_override: Option<(u16, u16)>,
        process_target: Option<crate::interceptor::ProcessTarget>,
    ) -> Result<(), String> {
        self.stop_interceptor();

        let game_box = crate::games::resolve(game_key).map_err(|e| e.to_string())?;

        // The GUI follows the client default: FEC on means the adaptive policy,
        // and an explicit opt-out keeps the fixed codec, matching the CLI paths.
        let adaptive_fec = crate::tunnel::adaptive::AdaptiveConfig {
            enabled: fec,
            ..Default::default()
        };

        // Precedence, authoritative: a picked process beats the ports path. It
        // is bound against the live process table at start time, so the image
        // name (not a stale PID) is what binds, and a target whose process is
        // not running fails honestly instead of degrading to a port range.
        let config = match process_target {
            Some(target) => {
                // Runs the ProcessScanner synchronously on the calling thread.
                let live = target
                    .name()
                    .and_then(|name| crate::interceptor::find_game_process(&[name]));
                let resolved =
                    match crate::interceptor::resolve_process_target(&target, live.as_ref()) {
                        Some(resolved) => resolved,
                        None => {
                            return Err(format!(
                                "process target '{}' is not running: start it (or pick a running \
                                 process) and try again",
                                target.describe()
                            ))
                        }
                    };
                let live_routes = live.map(|p| p.routes).unwrap_or_default();
                crate::interceptor::build_config_for_process_target(
                    game_box.as_ref(),
                    resolved,
                    live_routes,
                    proxy_addr,
                    fec,
                    fec_k,
                    adaptive_fec,
                )
            }
            // Build config: runs ProcessScanner synchronously on the calling thread.
            None => {
                let config = crate::interceptor::build_config_for_game_with_port_range(
                    game_box.as_ref(),
                    proxy_addr,
                    fec,
                    fec_k,
                    adaptive_fec,
                    port_range_override,
                )
                .ok_or_else(|| format!("Failed to build interceptor config for '{}'", game_key))?;

                // The config builder is pure and always returns a config, so the
                // "is it actually running" check belongs here, at the one place
                // that starts interception. Field verification selected Fortnite
                // on a machine that did not have it installed: the old code
                // started anyway, produced a near-total `1024-65535` UDP capture,
                // and showed the user a success state while capturing nothing.
                // Refusing with the real remedy is the honest outcome.
                if config.initial_routes.is_empty() {
                    let names = game_box.process_names().join(", ");
                    let found = find_pid_by_name(
                        game_box
                            .process_names()
                            .first()
                            .copied()
                            .unwrap_or_default(),
                    );
                    if found.is_none() {
                        return Err(format!(
                            "{} is not running (looked for {names}), so there is nothing to \
                             intercept. Start the game, or use 'Target a process' to pick a \
                             running program and relay all of its traffic.",
                            game_box.name()
                        ));
                    }
                }
                config
            }
        };

        let interceptor = crate::interceptor::create_interceptor();

        interceptor.check_availability()?;

        let game_name = game_box.name().to_string();
        let initial_server = match config.process_target.as_ref() {
            Some(target) => format!("Process {}", target.describe()),
            None => config
                .initial_routes
                .first()
                .map(|r| r.remote.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "Auto-detecting ({}-{})…",
                        config.port_range.0, config.port_range.1
                    )
                }),
        };
        let platform = interceptor.platform_name();
        let bypass_mode = config.bypass.mode.as_str();

        self.spawn_registration(proxy_addr);
        self.note_telemetry_proxy(proxy_addr);
        // The start line's label is computed BEFORE the config is moved into
        // `start()`: it names whatever is actually intercepted, which depends on
        // the target set on that config.
        let target_label = Self::interceptor_target_label(&config, &game_name);

        // `start()` must run inside a Tokio runtime context: the WinDivert
        // backend calls `tokio::spawn`/`spawn_blocking`/`Handle::current()`,
        // which panic with "there is no reactor running" from a non-Tokio
        // thread (e.g. the egui UI loop). `start()` is synchronous and only
        // spawns tasks, so `block_on` enters the context and returns at once.
        let handle = self
            .rt
            .block_on(async move { interceptor.start(config) })
            .map_err(|e| e.to_string())?;

        {
            let mut s = write_or_recover(&self.status);
            s.interceptor_active = true;
            s.interceptor_platform = platform;
            s.interceptor_server = initial_server;
            s.interceptor_intercepted = 0;
            s.interceptor_from_proxy = 0;
            s.interceptor_injected = 0;
            s.interceptor_errors = 0;
            s.interceptor_payloads_over_budget = 0;
            s.interceptor_error = None;
            s.bypass_mode = bypass_mode.to_string();
            s.bypass_allowed = 0;
            s.bypass_refused = 0;
            s.bypass_decisions = 0;
            s.bypass_flips = 0;
            s.bypass_pre_gate_rtt = 0;
            s.bypass_evaluations = 0;
            s.bypass_relay = 0;
            s.bypass_direct = 0;
            s.bypass_hold = 0;
            s.control_registered = false;
            s.session_token = None;
            s.registration_error = None;
        }

        tracing::info!(
            "🚀 Interceptor started for {} via {} (proxy {})",
            target_label,
            platform,
            proxy_addr,
        );

        self.interceptor_handle = Some(handle);
        Ok(())
    }

    /// Withhold tunnelling until a relay has been chosen for the game server.
    ///
    /// Returns whether the backend honoured it. `false` means this platform has
    /// no gate, so the caller must not assume traffic is being held: it would
    /// otherwise wait forever on a hold that never happened.
    pub fn hold_tunnelling(&mut self) -> bool {
        self.interceptor_handle
            .as_ref()
            .is_some_and(|h| h.set_tunnelling_enabled(false))
    }

    /// Permit tunnelling again, once a relay has been chosen for the server.
    pub fn release_tunnelling(&mut self) -> bool {
        self.stop_hold_watch();
        self.interceptor_handle
            .as_ref()
            .is_some_and(|h| h.set_tunnelling_enabled(true))
    }

    /// Release a hold as soon as `connected` says the relay is up, without
    /// waiting for the caller to notice.
    ///
    /// The GUI's own release runs once per frame, and a frame is the one thing a
    /// GUI starved of focus cannot promise: the hold outlived the relay it had
    /// connected, so a session stayed on the direct path while the hold's own
    /// two-minute fallback eventually fired. This watcher runs on the engine's
    /// runtime instead, where the relay's arrival (a frame loop at 100 ms) is
    /// the clock that matters.
    ///
    /// Returns whether a watcher was armed; `false` when this run is not held or
    /// the backend has no gate, in which case there is nothing to release.
    pub fn release_hold_when_connected(
        &mut self,
        connected: impl Fn() -> bool + Send + 'static,
    ) -> bool {
        // The switch first, so this borrow of the handle ends before the watcher
        // borrows `rt`: the caller drops its engine guard before this call (the
        // watcher reads the shared status on the engine runtime, and arming used
        // to deadlock against that when the guard was still held).
        let switch = self
            .interceptor_handle
            .as_ref()
            .and_then(|h| h.tunnelling_switch());
        let Some(switch) = switch else {
            return false;
        };
        self.stop_hold_watch();
        self.hold_watch = Some(self.rt.spawn(async move {
            // Liveness trace: the watcher started. A session with a hold and no
            // line from this task proves it never ran, which nothing else in the
            // log could distinguish from a hold that ended another way.
            tracing::info!(
                "Hold release watcher armed (relay already up: {})",
                connected()
            );
            while !connected() {
                tokio::time::sleep(Self::HOLD_CONNECT_POLL).await;
            }
            switch.store(true, Ordering::Relaxed);
            tracing::info!(
                "The relay connected while the hold was still waiting for the game server; \
                 releasing it so the match is not left on the direct path"
            );
        }));
        true
    }

    fn stop_hold_watch(&mut self) {
        if let Some(watch) = self.hold_watch.take() {
            watch.abort();
        }
    }

    /// What the start line names: the process a target set, else the game key.
    ///
    /// A process target is not the game key. The caller passes a fallback key only
    /// so the config has one, and printing that key told the user "Interceptor
    /// started for Fortnite" while the interceptor was relaying `chrome.exe`. What
    /// the line must name is whatever is actually being intercepted, so a set target
    /// wins and the key is the fallback for the built-in ports path.
    fn interceptor_target_label(config: &InterceptorConfig, game_name: &str) -> String {
        match config.process_target.as_ref() {
            Some(target) if target.is_set() => format!("process {}", target.describe()),
            _ => game_name.to_string(),
        }
    }

    /// Stop the OOP interceptor (if running), waiting a bounded time for the
    /// platform owner threads to release their handles.
    pub fn stop_interceptor(&mut self) {
        self.stop_supervised();
        if let Some(mut h) = self.interceptor_handle.take() {
            let stopped = h.stop_and_wait(Duration::from_millis(1500));
            if !stopped {
                tracing::warn!(
                    "Interceptor teardown did not complete within 1500ms; \
                     platform filter/handles may still be closing"
                );
            }
        }
        if let Ok(mut s) = self.status.write() {
            s.interceptor_active = false;
        }
    }

    // ─────────────────────────────────────────────────────────────────────

    /// A poisoned status lock does not brick snapshots; this is the same
    /// recovery for a caller that needs the live status rather than a copy.
    pub fn status_watch(&self) -> Arc<RwLock<EngineStatus>> {
        Arc::clone(&self.status)
    }

    /// Snapshot of current engine state, including live stats from all modes.
    pub fn snapshot(&self) -> EngineStatus {
        let mut snap = read_or_recover(&self.status).clone();

        // Overlay live redirect counters from atomics (avoids lock contention).
        if let Some(ref rs) = self.redirect_stats {
            snap.redirect_pkts_out = rs.packets_to_proxy.load(Ordering::Relaxed);
            snap.redirect_pkts_in = rs.packets_to_game.load(Ordering::Relaxed);
            snap.redirect_errors = rs.errors.load(Ordering::Relaxed);
            snap.redirect_fec_parity = rs.fec_parity_sent.load(Ordering::Relaxed);
            snap.redirect_fec_recovered = rs.fec_recovered.load(Ordering::Relaxed);
        }

        // Overlay live capture counters (filled by the capture task once running).
        if let Some(ref slot) = self.capture_stat_slot {
            if let Ok(guard) = slot.lock() {
                if let Some(ref h) = *guard {
                    snap.capture_pkts_out = h.outbound_packets.load(Ordering::Relaxed);
                    snap.capture_pkts_in =
                        h.injector_stats.packets_from_proxy.load(Ordering::Relaxed);
                    snap.capture_injected =
                        h.injector_stats.packets_injected.load(Ordering::Relaxed);
                    snap.capture_errors = h.injector_stats.inject_errors.load(Ordering::Relaxed);
                    snap.capture_fec_recovered =
                        h.injector_stats.fec_recovered.load(Ordering::Relaxed);
                }
            }
        }

        // Overlay live WinDivert counters (filled by the WinDivert task once running).
        if let Some(ref slot) = self.windivert_stat_slot {
            if let Ok(guard) = slot.lock() {
                if let Some(ref s) = *guard {
                    snap.windivert_intercepted = s.packets_intercepted.load(Ordering::Relaxed);
                    snap.windivert_from_proxy = s.packets_from_proxy.load(Ordering::Relaxed);
                    snap.windivert_injected = s.packets_injected.load(Ordering::Relaxed);
                    snap.windivert_errors = s.errors.load(Ordering::Relaxed);
                    // Update display string from auto-detected server address.
                    if let Ok(ds) = s.detected_server.lock() {
                        if let Some(addr) = *ds {
                            snap.windivert_server = addr.to_string();
                        }
                    }
                }
            }
        }

        // Overlay live OOP interceptor counters.
        if let Some(ref handle) = self.interceptor_handle {
            let ist = handle.snapshot();
            snap.interceptor_intercepted = ist.packets_intercepted;
            snap.interceptor_seen = ist.packets_seen;
            snap.interceptor_tunnel_sent = ist.packets_tunnel_sent;
            snap.capture_diagnosis = ist.capture_diagnosis.clone();
            snap.tunnelling_enabled = ist.tunnelling_enabled;
            snap.bypass_advantage_ms = ist.bypass_advantage_ms;
            snap.interceptor_from_proxy = ist.packets_from_proxy;
            snap.interceptor_injected = ist.packets_injected;
            snap.interceptor_errors = ist.errors;
            snap.interceptor_payloads_over_budget = ist.payloads_over_budget;
            snap.bypass_allowed = ist.bypass_allowed;
            snap.bypass_refused = ist.bypass_refused;
            snap.bypass_decisions = ist.bypass_decisions;
            snap.bypass_flips = ist.bypass_flips;
            snap.bypass_pre_gate_rtt = ist.bypass_pre_gate_rtt;
            snap.bypass_evaluations = ist.bypass_evaluations;
            snap.bypass_relay = ist.bypass_relay;
            snap.bypass_direct = ist.bypass_direct;
            snap.bypass_hold = ist.bypass_hold;
            snap.interceptor_platform = ist.platform;
            if let Some(srv) = ist.detected_server {
                snap.interceptor_server = srv.to_string();
            }
            snap.interceptor_active = handle.is_active();
        }

        snap
    }

    /// Access the shared-status handle (useful for background polling).
    pub fn status_arc(&self) -> Shared {
        Arc::clone(&self.status)
    }
}

/// Record a `register_session` outcome in `status` so the UI can show whether
/// the control plane accepted this client and which token it issued.
fn apply_registration_result(status: &mut EngineStatus, result: Option<u32>) {
    match result {
        Some(token) => {
            status.control_registered = true;
            status.session_token = Some(token);
            status.registration_error = None;
        }
        None => {
            status.control_registered = false;
            status.session_token = None;
            status.registration_error = Some("control-plane registration failed".to_string());
        }
    }
}

/// Bounded wait for the WinDivert redirect task's completion acknowledgement.
///
/// `None` means no redirect task is running, so there is nothing to wait for.
/// A disconnected channel means the task already ended without sending (for
/// example it panicked), so its handles are gone too. Only a live sender that
/// stays silent past `timeout` yields `false`.
fn wait_for_redirect_ack(ack: Option<std::sync::mpsc::Receiver<()>>, timeout: Duration) -> bool {
    match ack {
        Some(rx) => !matches!(
            rx.recv_timeout(timeout),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        None => true,
    }
}

impl Drop for LightSpeedEngine {
    fn drop(&mut self) {
        self.stop_interceptor();
        self.stop_windivert();
        self.stop_capture();
        self.stop_redirect();
        self.disconnect();
    }
}

// ── Background keepalive task ─────────────────────────────────────────────

async fn run_keepalive(
    proxy: SocketAddrV4,
    status: Shared,
    mut rx: oneshot::Receiver<()>,
    generation: u64,
) {
    let bind = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let socket = match UdpSocket::bind(bind).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("GUI engine: socket bind failed: {}", e);
            // Only mark disconnected if we're still the current generation.
            if let Ok(mut s) = status.write() {
                if s.keepalive_generation == generation {
                    s.connected = false;
                }
            }
            return;
        }
    };

    let mut ts: HashMap<u16, Instant> = HashMap::new();
    let mut seq: u16 = 0;
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    let mut buf = vec![0u8; 2048];

    loop {
        tokio::select! {
            biased;
            _ = &mut rx => break,
            _ = ticker.tick() => {
                let hdr = lightspeed_protocol::TunnelHeader::keepalive(
                    seq,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32,
                )
                .with_session_token(crate::session::path_token(proxy));
                if socket.send_to(&hdr.encode_to_array(), proxy).await.is_ok() {
                    ts.insert(seq, Instant::now());
                    ts.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                    if let Ok(mut s) = status.write() {
                        s.packets_sent += 1;
                    }
                }
                seq = seq.wrapping_add(1);
            }
            res = socket.recv_from(&mut buf) => {
                if let Ok((len, _)) = res {
                    if let Ok((hdr, _)) =
                        lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len])
                    {
                        if hdr.is_keepalive() {
                            if let Some(send_time) = ts.remove(&hdr.sequence) {
                                let rtt_ms = send_time.elapsed().as_secs_f64() * 1000.0;
                                if let Ok(mut s) = status.write() {
                                    s.packets_received += 1;
                                    s.latest_rtt_ms = rtt_ms;
                                    s.rtt_history.push(rtt_ms);
                                    if s.rtt_history.len() > 120 {
                                        s.rtt_history.remove(0);
                                    }
                                }
                                if crate::telemetry::is_enabled() {
                                    if let Some(collector) =
                                        crate::telemetry::paths::global_collector()
                                    {
                                        collector.record_rtt(rtt_ms).await;
                                        collector.record_path_rtt(
                                            &crate::telemetry::paths::relay_label(proxy),
                                            rtt_ms,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Only clear connected status if we are still the active generation.
    // If connect() was called again while we were running, new gen already
    // set connected=true and we must not clobber it.
    if let Ok(mut s) = status.write() {
        if s.keepalive_generation == generation {
            s.connected = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::{
        apply_registration_result, wait_for_redirect_ack, EngineStatus, LightSpeedEngine,
        DEFAULT_CONTROL_PORT,
    };
    use crate::interceptor::{
        mock::MockInterceptor, InterceptorConfig, ProcessTarget, TrafficInterceptor, TransportProto,
    };

    fn test_engine() -> LightSpeedEngine {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio runtime for engine test");
        LightSpeedEngine::new(rt.handle().clone())
    }

    #[test]
    fn a_poisoned_status_lock_does_not_brick_snapshots() {
        let engine = test_engine();

        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = engine.status.write().unwrap();
            panic!("simulated panic while holding the status write lock");
        }));
        assert!(poisoned.is_err(), "the panic must poison the lock");

        let snapshot = engine.snapshot();
        assert!(!snapshot.connected, "a fresh engine reports disconnected");
    }

    #[test]
    fn control_port_defaults_and_can_be_overridden() {
        let mut engine = test_engine();
        assert_eq!(engine.control_port(), DEFAULT_CONTROL_PORT);
        engine.set_control_port(8443);
        assert_eq!(engine.control_port(), 8443);
    }

    #[test]
    fn stop_windivert_returns_promptly_when_ack_already_fired() {
        let mut engine = test_engine();
        let (done_tx, done_rx) = mpsc::channel();
        done_tx.send(()).expect("send pre-fired ack");
        engine.windivert_done_rx = Some(done_rx);

        let started = Instant::now();
        engine.stop_windivert();

        assert!(
            started.elapsed() < Duration::from_millis(500),
            "an already-fired ack must not stall stop_windivert"
        );
    }

    #[test]
    fn stop_windivert_waits_for_delayed_ack() {
        let mut engine = test_engine();
        let (done_tx, done_rx) = mpsc::channel();
        engine.windivert_done_rx = Some(done_rx);

        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let _ = done_tx.send(());
        });

        let started = Instant::now();
        engine.stop_windivert();
        let elapsed = started.elapsed();
        worker.join().expect("ack thread must finish");

        assert!(
            elapsed >= Duration::from_millis(150),
            "stop_windivert returned in {elapsed:?} without waiting for the ack"
        );
    }

    #[test]
    fn wait_for_redirect_ack_reports_timeout_without_hanging() {
        let (_done_tx, done_rx) = mpsc::channel();
        let started = Instant::now();

        let completed = wait_for_redirect_ack(Some(done_rx), Duration::from_millis(80));

        assert!(!completed, "a silent live sender must time out");
        assert!(started.elapsed() >= Duration::from_millis(70));
    }

    #[test]
    fn wait_for_redirect_ack_without_task_is_immediate() {
        assert!(wait_for_redirect_ack(None, Duration::from_millis(80)));
    }

    #[test]
    fn engine_registration_success_records_token() {
        let mut status = EngineStatus::default();
        apply_registration_result(&mut status, Some(0xdead_beef));
        assert!(status.control_registered);
        assert_eq!(status.session_token, Some(0xdead_beef));
        assert_eq!(status.registration_error, None);
    }

    #[test]
    fn engine_registration_failure_records_error() {
        let mut status = EngineStatus::default();
        apply_registration_result(&mut status, None);
        assert!(!status.control_registered);
        assert_eq!(status.session_token, None);
        assert!(status.registration_error.is_some());
    }

    #[test]
    fn the_start_line_names_the_target_process_not_the_fallback_key() {
        // Field-verification regression: the interceptor logged "started for
        // Fortnite" while relaying chrome.exe, because it printed the fallback
        // game key instead of the picked process. The label must name what is
        // actually intercepted.
        let mut config = InterceptorConfig {
            game_name: "fortnite".into(),
            dynamic_server: false,
            transport: TransportProto::Udp,
            process_names: vec!["chrome.exe".to_string()],
            process_target: Some(ProcessTarget::by_name("chrome.exe")),
            port_range: (0, 0),
            initial_routes: Vec::new(),
            proxy_addr: std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(127, 0, 0, 1), 4434),
            fec_enabled: false,
            fec_k: 0,
            adaptive_fec: Default::default(),
            bypass: Default::default(),
        };

        assert_eq!(
            LightSpeedEngine::interceptor_target_label(&config, "fortnite"),
            "process chrome.exe"
        );

        // With a resolved PID the description carries it, so an operator can
        // tell which instance of a multi-process image is being intercepted.
        config.process_target = Some(ProcessTarget {
            name: Some("chrome.exe".into()),
            pid: Some(9348),
        });
        assert_eq!(
            LightSpeedEngine::interceptor_target_label(&config, "fortnite"),
            "process chrome.exe (PID 9348)"
        );

        // No target: the built-in ports path keeps naming its game key.
        config.process_target = None;
        assert_eq!(
            LightSpeedEngine::interceptor_target_label(&config, "fortnite"),
            "fortnite"
        );

        // An empty target is not a target, so the key still names the run.
        config.process_target = Some(ProcessTarget::default());
        assert_eq!(
            LightSpeedEngine::interceptor_target_label(&config, "fortnite"),
            "fortnite"
        );
    }

    #[test]
    fn release_hold_when_connected_is_armed_only_for_a_gated_run() {
        // Contract: `false` means nothing is being held, so a caller must not
        // assume a release is coming. The mock has no gate.
        let mut engine = test_engine();
        assert!(
            !engine.release_hold_when_connected(|| false),
            "an engine with no interceptor has nothing to hold"
        );

        let handle = MockInterceptor::new()
            .start(InterceptorConfig {
                game_name: "TestGame".into(),
                dynamic_server: false,
                transport: TransportProto::Udp,
                process_names: vec!["TestGame.exe".to_string()],
                process_target: Some(ProcessTarget::by_name("TestGame.exe")),
                port_range: (27015, 27017),
                initial_routes: Vec::new(),
                proxy_addr: std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::new(127, 0, 0, 1),
                    4434,
                ),
                fec_enabled: false,
                fec_k: 4,
                adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig::default(),
                bypass: Default::default(),
            })
            .expect("mock interceptor starts");
        engine.interceptor_handle = Some(handle);

        assert!(
            !engine.release_hold_when_connected(|| true),
            "a backend with no tunnelling gate must report that it cannot release a hold"
        );
        assert!(
            !engine.hold_tunnelling(),
            "the mock has no gate, so holding traffic is reported as not applied"
        );
    }

    #[test]
    fn snapshot_surfaces_bypass_evaluations_from_the_interceptor() {
        let mut engine = test_engine();
        let handle = MockInterceptor::new()
            .start(InterceptorConfig {
                game_name: "TestGame".into(),
                dynamic_server: false,
                transport: TransportProto::Udp,
                process_names: vec!["TestGame.exe".to_string()],
                process_target: Some(ProcessTarget::by_name("TestGame.exe")),
                port_range: (27015, 27017),
                initial_routes: Vec::new(),
                proxy_addr: std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::new(127, 0, 0, 1),
                    4434,
                ),
                fec_enabled: false,
                fec_k: 4,
                // The client default is adaptive FEC on; the interceptor config mirrors it.
                adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig {
                    enabled: true,
                    ..Default::default()
                },
                bypass: Default::default(),
            })
            .expect("mock interceptor starts");
        handle
            .counters
            .bypass_evaluations
            .fetch_add(3, Ordering::Relaxed);
        engine.interceptor_handle = Some(handle);

        assert_eq!(
            engine.snapshot().bypass_evaluations,
            3,
            "the engine snapshot must surface the interceptor's steady-state count"
        );
    }

    /// A picked process that is not running is reported, never silently
    /// degraded to the port-range path.
    #[test]
    fn a_missing_process_target_is_reported_not_silently_ignored() {
        let mut engine = test_engine();

        let err = engine
            .start_interceptor_with_target(
                "rust",
                "127.0.0.1:4434".parse().unwrap(),
                false,
                4,
                None,
                Some(ProcessTarget::by_name("lightspeed_no_such_process_xyz.exe")),
            )
            .expect_err("a named process that is not running must fail, not fall back to ports");

        assert!(
            err.contains("not running"),
            "the error must name the missing process honestly, got: {err}"
        );
        assert!(
            !engine.snapshot().interceptor_active,
            "a failed start must not report an active interceptor"
        );
    }

    #[test]
    fn a_built_in_game_that_is_not_running_is_refused_not_started() {
        // Field-verification regression: the owner selected a built-in game on a
        // machine that did not have it installed. The app started anyway, ran a
        // near-total `1024-65535` UDP capture, and showed a success state while
        // nothing was being intercepted - worse than an error, because it spends
        // the machine's resources and teaches the user to distrust the tool.
        //
        // The refusal lives at the start path rather than in the config builder:
        // the builder stays pure so a dry run or a port preview still works.
        let mut engine = test_engine();

        let err = engine
            .start_interceptor(
                // A real profile whose process is certainly not running here.
                "hunt",
                "127.0.0.1:4434".parse().unwrap(),
                false,
                4,
                None,
            )
            .expect_err("a built-in game that is not running must be refused");

        assert!(
            err.contains("not running"),
            "the error must say the game is not running, got: {err}"
        );
        assert!(
            err.contains("Target a process"),
            "the error must name the working alternative, got: {err}"
        );
        assert!(
            !engine.snapshot().interceptor_active,
            "a refused start must not report an active interceptor"
        );
    }
}
