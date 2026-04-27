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
}

// ── Engine ───────────────────────────────────────────────────────────────

type Shared = Arc<Mutex<EngineStatus>>;

/// Manages the keepalive loop and optional game redirect.
/// Designed for use from GUI code running outside a Tokio context.
pub struct LightSpeedEngine {
    rt: Handle,
    status: Shared,
    shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_shutdown_tx: Option<oneshot::Sender<()>>,
    redirect_stats: Option<Arc<RedirectStats>>,
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

    /// Snapshot of current engine state, including live redirect stats.
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
        snap
    }

    /// Access the shared-status handle (useful for background polling).
    pub fn status_arc(&self) -> Shared {
        Arc::clone(&self.status)
    }
}

impl Drop for LightSpeedEngine {
    fn drop(&mut self) {
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
