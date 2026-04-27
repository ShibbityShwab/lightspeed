//! Background keepalive engine — drives the tunnel loop from a GUI context.
//!
//! [`LightSpeedEngine`] can be created on any thread.  It spawns async tasks
//! onto the provided Tokio [`Handle`] and updates a shared [`EngineStatus`]
//! that the GUI polls on every frame.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

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
}

// ── Engine ───────────────────────────────────────────────────────────────

type Shared = Arc<Mutex<EngineStatus>>;

/// Manages the background keepalive loop and exposes a status snapshot.
///
/// Designed for use from GUI code running outside a Tokio context.
/// Pass a [`Handle`] pointing at a background runtime:
///
/// ```ignore
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// let engine = LightSpeedEngine::new(rt.handle().clone());
/// ```
pub struct LightSpeedEngine {
    rt: Handle,
    status: Shared,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl LightSpeedEngine {
    /// Create a new (disconnected) engine backed by `rt`.
    pub fn new(rt: Handle) -> Self {
        Self {
            rt,
            status: Arc::new(Mutex::new(EngineStatus::default())),
            shutdown_tx: None,
        }
    }

    /// Start (or restart) the keepalive loop toward `proxy_addr`.
    pub fn connect(&mut self, proxy_addr: SocketAddrV4) {
        self.disconnect();
        let (tx, rx) = oneshot::channel();
        self.shutdown_tx = Some(tx);
        {
            let mut s = self.status.lock().unwrap();
            s.connected = true;
            s.proxy_addr = proxy_addr.to_string();
            s.packets_sent = 0;
            s.packets_received = 0;
            s.rtt_history.clear();
            s.latest_rtt_ms = 0.0;
        }
        let status = Arc::clone(&self.status);
        self.rt.spawn(run_keepalive(proxy_addr, status, rx));
    }

    /// Stop the tunnel loop.
    pub fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Ok(mut s) = self.status.lock() {
            s.connected = false;
        }
    }

    /// Snapshot of current engine state.  Cheap clone of inner mutex.
    pub fn snapshot(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }

    /// Access the shared-status handle (useful for background polling).
    pub fn status_arc(&self) -> Shared {
        Arc::clone(&self.status)
    }
}

impl Drop for LightSpeedEngine {
    fn drop(&mut self) {
        self.disconnect();
    }
}

// ── Background keepalive task ─────────────────────────────────────────────

async fn run_keepalive(proxy: SocketAddrV4, status: Shared, mut rx: oneshot::Receiver<()>) {
    let bind = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let socket = match UdpSocket::bind(bind).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("GUI engine: socket bind failed: {}", e);
            if let Ok(mut s) = status.lock() {
                s.connected = false;
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

    if let Ok(mut s) = status.lock() {
        s.connected = false;
    }
}
