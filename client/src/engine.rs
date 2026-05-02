//! Background keepalive + redirect engine — drives the tunnel loop from a GUI context.
//!
//! [`LightSpeedEngine`] can be created on any thread.  It spawns async tasks
//! onto the provided Tokio [`Handle`] and updates a shared [`EngineStatus`]
//! that the GUI polls on every frame.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::modes::capture_mode::CaptureStatSlot;
use crate::modes::redirect_windivert::WinDivertStatSlot;
use crate::redirect::{RedirectStats, UdpRedirect};

// ── Public types ─────────────────────────────────────────────────────────

/// Snapshot of tunnel state — cheap to clone, safe to share.
#[derive(Clone, Debug, Default)]
pub struct EngineStatus {
    /// Whether the keepalive loop is actively running.
    pub connected: bool,
    /// Proxy address as a display string, e.g. `"149.28.84.139:4434"`.
    pub proxy_addr: String,
    /// Latest round-trip time in milliseconds (0.0 when no measurement).
    pub latest_rtt_ms: f64,
    /// Rolling window of up to 120 RTT measurements (ms), newest last.
    pub rtt_history: Vec<f64>,
    /// Total keepalive packets sent this session.
    pub packets_sent: u64,
    /// Total keepalive echo packets received this session.
    pub packets_received: u64,
    /// Generation counter — incremented each time `connect()` is called.
    /// Used to prevent old tasks from overwriting status after reconnect.
    pub keepalive_generation: u64,

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
    /// Error description if the interceptor failed to start.
    pub interceptor_error: Option<String>,
}

// ── Engine ───────────────────────────────────────────────────────────────

type Shared = Arc<Mutex<EngineStatus>>;

/// Manages the keepalive loop, optional game redirect, and optional pcap capture.
/// Designed for use from GUI code running outside a Tokio context.
pub struct LightSpeedEngine {
    rt: Handle,
    status: Shared,
    shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_stats: Option<Arc<RedirectStats>>,
    /// Shutdown sender for active capture task.
    capture_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Slot filled by the capture task with live stat Arc handles.
    capture_stat_slot: Option<CaptureStatSlot>,
    /// Shutdown sender for active WinDivert redirect task.
    windivert_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Slot filled by the WinDivert task with live stat Arc handles.
    windivert_stat_slot: Option<WinDivertStatSlot>,
    /// Live handle for the OOP TrafficInterceptor (multiplatform MITM).
    interceptor_handle: Option<crate::interceptor::InterceptorHandle>,
}

impl LightSpeedEngine {
    /// Create a new (disconnected) engine backed by `rt`.
    pub fn new(rt: Handle) -> Self {
        Self {
            rt,
            status: Arc::new(Mutex::new(EngineStatus::default())),
            shutdown_tx: None,
            redirect_shutdown_tx: None,
            redirect_stats: None,
            capture_shutdown_tx: None,
            capture_stat_slot: None,
            windivert_shutdown_tx: None,
            windivert_stat_slot: None,
            interceptor_handle: None,
        }
    }

    /// Start (or restart) the keepalive loop toward `proxy_addr`.
    pub fn connect(&mut self, proxy_addr: SocketAddrV4) {
        self.disconnect();
        let (tx, rx) = oneshot::channel();
        self.shutdown_tx = Some(tx);
        let generation = {
            let mut s = self.status.lock().unwrap();
            s.keepalive_generation = s.keepalive_generation.wrapping_add(1);
            s.connected = true;
            s.proxy_addr = proxy_addr.to_string();
            s.packets_sent = 0;
            s.packets_received = 0;
            s.rtt_history.clear();
            s.latest_rtt_ms = 0.0;
            s.keepalive_generation
        };
        let status = Arc::clone(&self.status);
        self.rt.spawn(run_keepalive(proxy_addr, status, rx, generation));
    }

    /// Stop the keepalive loop.
    pub fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Ok(mut s) = self.status.lock() {
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
            let mut s = self.status.lock().unwrap();
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
        }

        let status = Arc::clone(&self.status);
        self.rt.spawn(async move {
            // run_with_shutdown takes &self and is Send, but UdpRedirect isn't
            // Arc'd — we move it into the task.
            match redirect.run_with_shutdown(rx).await {
                Ok(()) => {
                    tracing::info!("Redirect stopped cleanly");
                }
                Err(e) => {
                    tracing::error!("Redirect error: {}", e);
                    if let Ok(mut s) = status.lock() {
                        s.redirect_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.lock() {
                s.redirect_active = false;
            }
        });
    }

    /// Stop the game redirect (if running).
    pub fn stop_redirect(&mut self) {
        if let Some(tx) = self.redirect_shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.redirect_stats = None;
        if let Ok(mut s) = self.status.lock() {
            s.redirect_active = false;
        }
    }

    // ── Capture (pcap) mode ───────────────────────────────────────────────

    /// Start transparent pcap capture mode for the given game key.
    ///
    /// Sniffs game UDP directly from the NIC, tunnels through `proxy_addr`,
    /// and injects responses back — no server IP or game console command needed.
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

        // Resolve game config from key string.
        let game_box = crate::games::detect_game(game_key).map_err(|e| e.to_string())?;

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
        let resolved_interface = interface_opt
            .or_else(crate::capture::pick_best_interface);
        tracing::info!(
            "🖧 Capture interface: {}",
            resolved_interface.as_deref().unwrap_or("(none found)")
        );

        // Create the stat slot — the capture task will fill it once running.
        let stat_slot: CaptureStatSlot = Arc::new(Mutex::new(None));
        self.capture_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.capture_shutdown_tx = Some(tx);

        {
            let mut s = self.status.lock().unwrap();
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
        }

        let status = Arc::clone(&self.status);
        let proxy_id = proxy_addr.to_string();

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
                    if let Ok(mut s) = status.lock() {
                        s.capture_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.lock() {
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
        self.capture_stat_slot = None;
        if let Ok(mut s) = self.status.lock() {
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
    /// `proxy_addr`, and injects spoofed responses back — severs the game's
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
        self.stop_windivert();

        use crate::capture::windivert_redirect::WinDivertConfig;
        use crate::modes::redirect_windivert::run_windivert_mode_with_shutdown;

        let cfg = WinDivertConfig {
            server_addr: Some(server_addr),
            port_range: (server_addr.port(), server_addr.port()),
            proxy_addr,
            fec_enabled: fec,
            fec_k,
        };

        let stat_slot: WinDivertStatSlot = Arc::new(Mutex::new(None));
        self.windivert_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.windivert_shutdown_tx = Some(tx);

        {
            let mut s = self.status.lock().unwrap();
            s.windivert_active = true;
            s.windivert_server = server_addr.to_string();
            s.windivert_intercepted = 0;
            s.windivert_from_proxy = 0;
            s.windivert_injected = 0;
            s.windivert_errors = 0;
            s.windivert_error = None;
        }

        let status = Arc::clone(&self.status);
        self.rt.spawn(async move {
            match run_windivert_mode_with_shutdown(cfg, rx, Some(stat_slot)).await {
                Ok(()) => tracing::info!("WinDivert redirect stopped cleanly"),
                Err(e) => {
                    tracing::error!("WinDivert redirect error: {}", e);
                    if let Ok(mut s) = status.lock() {
                        s.windivert_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.lock() {
                s.windivert_active = false;
            }
        });

        Ok(())
    }

    /// Stub for non-Windows / feature-disabled builds — always returns `Err`.
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

    /// Start WinDivert kernel-level interception using a game's port range.
    ///
    /// No server IP is required — the first outbound Game UDP packet whose
    /// destination port falls in `[port_lo, port_hi]` is treated as the game
    /// server.  All subsequent game packets are tunnelled through `proxy_addr`.
    /// Non-game traffic that accidentally passes the broad filter is re-injected
    /// untouched.
    ///
    /// This is the ExitLag-style "just click Start, then launch your game"
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
        self.stop_windivert();

        use crate::capture::windivert_redirect::WinDivertConfig;
        use crate::modes::redirect_windivert::run_windivert_mode_with_shutdown;

        let cfg = WinDivertConfig {
            server_addr: None, // auto-detect from first packet
            port_range: (port_lo, port_hi),
            proxy_addr,
            fec_enabled: fec,
            fec_k,
        };

        let stat_slot: WinDivertStatSlot = Arc::new(Mutex::new(None));
        self.windivert_stat_slot = Some(Arc::clone(&stat_slot));

        let (tx, rx) = oneshot::channel();
        self.windivert_shutdown_tx = Some(tx);

        {
            let mut s = self.status.lock().unwrap();
            s.windivert_active = true;
            s.windivert_server = format!("Auto-detecting (ports {}-{})…", port_lo, port_hi);
            s.windivert_intercepted = 0;
            s.windivert_from_proxy = 0;
            s.windivert_injected = 0;
            s.windivert_errors = 0;
            s.windivert_error = None;
        }

        let status = Arc::clone(&self.status);
        self.rt.spawn(async move {
            match run_windivert_mode_with_shutdown(cfg, rx, Some(stat_slot)).await {
                Ok(()) => tracing::info!("WinDivert auto-redirect stopped cleanly"),
                Err(e) => {
                    tracing::error!("WinDivert auto-redirect error: {}", e);
                    if let Ok(mut s) = status.lock() {
                        s.windivert_error = Some(format!("{e}"));
                    }
                }
            }
            if let Ok(mut s) = status.lock() {
                s.windivert_active = false;
            }
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

    /// Stop the WinDivert redirect task (if running).
    pub fn stop_windivert(&mut self) {
        if let Some(tx) = self.windivert_shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.windivert_stat_slot = None;
        if let Ok(mut s) = self.status.lock() {
            s.windivert_active = false;
        }
    }

    // ── OOP TrafficInterceptor (multiplatform MITM) ───────────────────────

    /// Start the OOP `TrafficInterceptor` for a game identified by `game_key`.
    ///
    /// This is the **recommended** entry point for all new integrations:
    ///
    /// 1. Resolves the game profile.
    /// 2. Runs the `ProcessScanner` to discover the game PID + live server routes.
    /// 3. Creates the best available interceptor for the current OS.
    /// 4. Starts kernel-level MITM interception automatically.
    ///
    /// On Windows this uses WinDivert with a per-process PID filter — zero false
    /// positives even on shared game-server ports.
    /// On Linux/macOS it uses nftables/pfctl with a server-specific redirect rule.
    ///
    /// Returns `Err(msg)` synchronously if the game key is unknown, the
    /// interceptor is unavailable, or the start call fails.
    pub fn start_interceptor(
        &mut self,
        game_key: &str,
        proxy_addr: SocketAddrV4,
        fec: bool,
        fec_k: u8,
    ) -> Result<(), String> {
        self.stop_interceptor();

        let game_box = crate::games::detect_game(game_key)
            .map_err(|e| e.to_string())?;

        // Build config: runs ProcessScanner synchronously on the calling thread.
        let config = crate::interceptor::build_config_for_game(
            game_box.as_ref(),
            proxy_addr,
            fec,
            fec_k,
        )
        .ok_or_else(|| format!("Failed to build interceptor config for '{}'", game_key))?;

        let interceptor = crate::interceptor::create_interceptor();

        interceptor.check_availability()?;

        let game_name = game_box.name().to_string();
        let initial_server = config
            .initial_routes
            .first()
            .map(|r| r.remote.to_string())
            .unwrap_or_else(|| format!("Auto-detecting ({}-{})…",
                config.port_range.0, config.port_range.1));
        let platform = interceptor.platform_name();

        let handle = interceptor.start(config).map_err(|e| e.to_string())?;

        {
            let mut s = self.status.lock().unwrap();
            s.interceptor_active = true;
            s.interceptor_platform = platform;
            s.interceptor_server = initial_server;
            s.interceptor_intercepted = 0;
            s.interceptor_from_proxy = 0;
            s.interceptor_injected = 0;
            s.interceptor_errors = 0;
            s.interceptor_error = None;
        }

        tracing::info!(
            "🚀 Interceptor started for {} via {} (proxy {})",
            game_name,
            platform,
            proxy_addr,
        );

        self.interceptor_handle = Some(handle);
        Ok(())
    }

    /// Stop the OOP interceptor (if running).
    pub fn stop_interceptor(&mut self) {
        if let Some(mut h) = self.interceptor_handle.take() {
            h.stop();
        }
        if let Ok(mut s) = self.status.lock() {
            s.interceptor_active = false;
        }
    }

    // ─────────────────────────────────────────────────────────────────────

    /// Snapshot of current engine state, including live stats from all modes.
    pub fn snapshot(&self) -> EngineStatus {
        let mut snap = self.status.lock().unwrap().clone();

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
                    snap.capture_errors =
                        h.injector_stats.inject_errors.load(Ordering::Relaxed);
                    snap.capture_fec_recovered =
                        h.injector_stats.fec_recovered.load(Ordering::Relaxed);
                }
            }
        }

        // Overlay live WinDivert counters (filled by the WinDivert task once running).
        if let Some(ref slot) = self.windivert_stat_slot {
            if let Ok(guard) = slot.lock() {
                if let Some(ref s) = *guard {
                    snap.windivert_intercepted =
                        s.packets_intercepted.load(Ordering::Relaxed);
                    snap.windivert_from_proxy =
                        s.packets_from_proxy.load(Ordering::Relaxed);
                    snap.windivert_injected =
                        s.packets_injected.load(Ordering::Relaxed);
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
            snap.interceptor_from_proxy   = ist.packets_from_proxy;
            snap.interceptor_injected     = ist.packets_injected;
            snap.interceptor_errors       = ist.errors;
            snap.interceptor_platform     = ist.platform;
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
            if let Ok(mut s) = status.lock() {
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
                );
                if socket.send_to(&hdr.encode_to_array(), proxy).await.is_ok() {
                    ts.insert(seq, Instant::now());
                    ts.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                    if let Ok(mut s) = status.lock() {
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
                                if let Ok(mut s) = status.lock() {
                                    s.packets_received += 1;
                                    s.latest_rtt_ms = rtt_ms;
                                    s.rtt_history.push(rtt_ms);
                                    if s.rtt_history.len() > 120 {
                                        s.rtt_history.remove(0);
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
    if let Ok(mut s) = status.lock() {
        if s.keepalive_generation == generation {
            s.connected = false;
        }
    }
}
