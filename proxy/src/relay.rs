//! # Proxy Packet Relay
//!
//! Core relay engine: receives tunnel packets from clients, strips header,
//! forwards to game server, receives response, re-wraps, returns to client.
//!
//! ## Linux recvmmsg batched I/O (Item K)
//!
//! On Linux the inbound receive path uses `recvmmsg(2)` to drain up to 32
//! packets per syscall, reducing per-packet syscall overhead by ~10–30× at
//! high packet rates.  A [`BatchState`] struct holds the kernel-facing buffers
//! and is re-initialised on every call to avoid self-referential pointer
//! issues.  The non-Linux path falls back to a single `recv_from` per packet.
//!
//! ## FEC Support
//!
//! When a client sends version-2 packets, FEC is active:
//! - **Inbound**: Data packets have FEC header stripped before forwarding.
//!   Parity packets are absorbed (not forwarded) and used for recovery.
//! - **Outbound**: Responses are FEC-encoded with the same K for the client.
//!
//! ## Security
//!
//! The relay enforces multiple security layers:
//! 1. **Authentication**: Validates (IP + session_token) per-packet
//! 2. **Rate limiting**: Per-client PPS and BPS limits
//! 3. **Abuse detection**: Amplification and reflection detection
//! 4. **Destination validation**: Blocks forwarding to private/internal IPs

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use lightspeed_protocol::framing::{read_frame, write_frame, MAX_FRAME_SIZE};
use lightspeed_protocol::{
    build_fec_parity_packet, build_tcp_packet, decode_tcp_payload, tcp_flags, FecDecoder,
    FecEncoder, FecHeader, Tcp5Tuple, TcpExtHeader, TcpReliable, TunnelHeader, FEC_HEADER_SIZE,
    FEC_MAX_PAYLOAD, HEADER_SIZE,
};

/// Maximum size for a single outbound relay packet:
/// TunnelHeader (20 B) + FecHeader (4 B) + receive buffer (2048 B).
/// Declared as a module-level constant so the stack array in the response
/// listener task is sized at compile time.
const MAX_RELAY_PKT: usize = HEADER_SIZE + FEC_HEADER_SIZE + 2048;

/// While a session's server-leg interest is live, refresh it once per this many
/// successful forwards, so the registry's interest TTL cannot expire under a
/// slow but active session without a lock on every packet.
const SERVER_LEG_REFRESH_PACKETS: u64 = 32;

/// Slots in one TCP connection's client→server command channel.
///
/// The channel is small on purpose: the inbound path must never wait on a game
/// server that has stopped reading. The gate in [`process_tcp_packet`] refuses
/// new segments once every slot is taken, before the reliability receiver sees
/// them, so the client's retransmit timer recovers the refused bytes.
const TCP_CMD_QUEUE_SLOTS: usize = 64;

/// Byte budget for one TCP connection's client→server command queue (~256 KiB).
///
/// The slot cap alone bounds a queue of full-sized segments; this bounds a
/// queue whose batches each carry many chunks released from the reorder
/// window. It is per connection, never global, so one stalled game server
/// cannot consume another connection's budget.
const TCP_CMD_QUEUE_BYTES: usize = 256 * 1024;

/// How long the connection task may spend writing one queued chunk upstream
/// without completing it. A game server that stops reading must cost at most
/// this connection: on expiry the task resets only this connection.
const TCP_WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(60);

use super::abuse::{AbuseCheckResult, AbuseDetector};
use super::auth::Authenticator;
use super::budget::{BudgetExceeded, BudgetGuard};
use super::geo::{GeoResolver, GeoSide};
#[cfg(target_os = "linux")]
use super::handoff::SessionSnapshot;
use super::metrics::{upstream_lag, DropReason, ProxyMetrics};
use super::ping::{passive_app_rtt, ServerLegRegistry, FORWARD_THRESHOLD};
use super::rate_limit::{RateLimitResult, RateLimiter};
use super::trace::TraceRegistry;

// ─────────────────────────────────────────────────────────────────────────────
//  Socket hygiene: bounded buffers and Linux receive busy-poll
// ─────────────────────────────────────────────────────────────────────────────
//
// Three deliberate decisions, each of which degrades to a no-op rather than
// disturbing the relay:
//
// 1. `SO_RCVBUF`/`SO_SNDBUF` are set explicitly and bounded. Kernel defaults
//    (often ~208 KiB) are sized for general hosts; a relay draining bursty game
//    traffic can drop datagrams when its task is descheduled and the receive
//    queue overflows. The data socket gets a larger request because it carries
//    every client; each per-session egress socket gets a smaller one because
//    hundreds can exist at once, so per-socket memory scales with sessions.
//    Values are clamped to [`SOCK_BUF_MIN`]..[`SOCK_BUF_MAX`]. The kernel still
//    caps the result at `net.core.rmem_max`/`wmem_max`, so the readback below,
//    not the request, is the truth that gets logged.
//
// 2. On Linux, `SO_BUSY_POLL` asks the kernel to spin briefly (50 us) on a
//    socket's receive path after a wakeup, trading CPU for lower wakeup
//    latency. It is advisory: with the common default `net.core.busy_poll=0`
//    it is inert, so on a default host it costs nothing and buys nothing. This
//    is the honest cost of the option: on a host that has enabled global busy
//    polling it burns CPU, which is why the duration is small and the option is
//    only requested where a kernel supports it (an unsupported kernel returns
//    ENOPROTOOPT and the relay carries on).
//
// 3. `sendmmsg` is deliberately NOT used. Responses are emitted one datagram at
//    a time from a per-session Tokio `UdpSocket`, so there is no batching point
//    to feed a multi-message send. Issuing raw `sendmmsg` on a Tokio-managed fd
//    would also bypass the reactor's send-readiness tracking, risking a lost
//    wakeup or a busy-loop on the exact path that must never stall. The
//    `recvmmsg` inbound path is unchanged and remains the only batched syscall.

/// Lower bound for any requested socket buffer.
const SOCK_BUF_MIN: usize = 64 * 1024;
/// Upper bound, so a bad value cannot ask for gigabytes per socket.
const SOCK_BUF_MAX: usize = 16 * 1024 * 1024;
/// Aggregate data-plane socket buffer target.
const DATA_SOCKET_RCVBUF: usize = 4 * 1024 * 1024;
const DATA_SOCKET_SNDBUF: usize = 4 * 1024 * 1024;
/// Per-session game-server socket buffer target.
const SESSION_SOCKET_RCVBUF: usize = 512 * 1024;
const SESSION_SOCKET_SNDBUF: usize = 512 * 1024;
/// Linux `SO_BUSY_POLL` duration, in microseconds.
#[cfg(target_os = "linux")]
const BUSY_POLL_US: u32 = 50;

/// Which relay socket is being tuned; selects the buffer policy.
#[derive(Clone, Copy, Debug)]
enum SocketRole {
    Data,
    Session,
}

impl SocketRole {
    fn rcvbuf(self) -> usize {
        match self {
            Self::Data => DATA_SOCKET_RCVBUF,
            Self::Session => SESSION_SOCKET_RCVBUF,
        }
    }

    fn sndbuf(self) -> usize {
        match self {
            Self::Data => DATA_SOCKET_SNDBUF,
            Self::Session => SESSION_SOCKET_SNDBUF,
        }
    }
}

/// What actually happened when a socket was tuned. `*_effective` is the kernel's
/// readback (after its own cap/doubling), `None` when the option was refused.
#[derive(Debug, Clone, Copy, Default)]
struct SocketTuningReport {
    rcvbuf_wanted: u32,
    rcvbuf_effective: Option<u32>,
    sndbuf_wanted: u32,
    sndbuf_effective: Option<u32>,
    busy_poll: Option<u32>,
    failures: u32,
}

/// Clamp a requested buffer size into the sane window.
fn clamp_buf(bytes: usize) -> u32 {
    bytes.clamp(SOCK_BUF_MIN, SOCK_BUF_MAX) as u32
}

#[cfg(target_os = "linux")]
fn set_int_opt(
    fd: libc::c_int,
    level: libc::c_int,
    name: libc::c_int,
    value: libc::c_int,
) -> std::io::Result<()> {
    // SAFETY: `value` is a live c_int and the length is exactly its size, so
    // setsockopt reads only initialised bytes for the duration of the call.
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            std::ptr::addr_of!(value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn get_int_opt(
    fd: libc::c_int,
    level: libc::c_int,
    name: libc::c_int,
) -> std::io::Result<libc::c_int> {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `value` and `len` are valid out-pointers, and the kernel writes
    // at most `len` initialised bytes into them.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            level,
            name,
            std::ptr::addr_of_mut!(value).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(value)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Apply bounded buffers and, on Linux, busy-poll to an already-bound fd.
///
/// Every step is independent: a refused option increments `failures` and the
/// caller's metrics but never aborts the socket. `fd = -1` exercises the whole
/// failure path in tests.
#[cfg(target_os = "linux")]
fn configure_socket_fd(
    fd: libc::c_int,
    role: SocketRole,
    metrics: &ProxyMetrics,
) -> SocketTuningReport {
    let mut report = SocketTuningReport {
        rcvbuf_wanted: clamp_buf(role.rcvbuf()),
        sndbuf_wanted: clamp_buf(role.sndbuf()),
        ..Default::default()
    };

    match set_int_opt(
        fd,
        libc::SOL_SOCKET,
        libc::SO_RCVBUF,
        report.rcvbuf_wanted as libc::c_int,
    ) {
        Ok(()) => {
            metrics.record_socket_option(true);
            report.rcvbuf_effective = get_int_opt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF)
                .ok()
                .map(|v| v as u32);
        }
        Err(e) => {
            metrics.record_socket_option(false);
            report.failures += 1;
            debug!(error = %e, role = ?role, "SO_RCVBUF refused; using kernel default");
        }
    }

    match set_int_opt(
        fd,
        libc::SOL_SOCKET,
        libc::SO_SNDBUF,
        report.sndbuf_wanted as libc::c_int,
    ) {
        Ok(()) => {
            metrics.record_socket_option(true);
            report.sndbuf_effective = get_int_opt(fd, libc::SOL_SOCKET, libc::SO_SNDBUF)
                .ok()
                .map(|v| v as u32);
        }
        Err(e) => {
            metrics.record_socket_option(false);
            report.failures += 1;
            debug!(error = %e, role = ?role, "SO_SNDBUF refused; using kernel default");
        }
    }

    match set_int_opt(
        fd,
        libc::SOL_SOCKET,
        libc::SO_BUSY_POLL,
        BUSY_POLL_US as libc::c_int,
    ) {
        Ok(()) => {
            metrics.record_socket_option(true);
            metrics.record_busy_poll_enabled();
            report.busy_poll = Some(BUSY_POLL_US);
        }
        Err(e) => {
            metrics.record_socket_option(false);
            report.failures += 1;
            // Expected on kernels without SO_BUSY_POLL; not a problem.
            debug!(error = %e, role = ?role, "SO_BUSY_POLL unavailable");
        }
    }

    report
}

/// Tune a bound Tokio UDP socket. On Linux this really calls the kernel; on
/// other targets it reports the requested sizes only.
#[cfg(target_os = "linux")]
fn configure_socket(
    socket: &UdpSocket,
    role: SocketRole,
    metrics: &ProxyMetrics,
) -> SocketTuningReport {
    use std::os::fd::AsRawFd;
    configure_socket_fd(socket.as_raw_fd(), role, metrics)
}

#[cfg(not(target_os = "linux"))]
fn configure_socket(
    socket: &UdpSocket,
    role: SocketRole,
    metrics: &ProxyMetrics,
) -> SocketTuningReport {
    let _ = (socket, metrics);
    SocketTuningReport {
        rcvbuf_wanted: clamp_buf(role.rcvbuf()),
        sndbuf_wanted: clamp_buf(role.sndbuf()),
        ..Default::default()
    }
}

/// Tune and report the single aggregate data-plane socket at loop start.
fn configure_data_socket(data_socket: &UdpSocket, metrics: &ProxyMetrics) {
    let report = configure_socket(data_socket, SocketRole::Data, metrics);
    info!(
        role = "data",
        rcvbuf_wanted = report.rcvbuf_wanted,
        rcvbuf_effective = ?report.rcvbuf_effective,
        sndbuf_wanted = report.sndbuf_wanted,
        sndbuf_effective = ?report.sndbuf_effective,
        busy_poll_us = ?report.busy_poll,
        refusals = report.failures,
        "Data socket tuned"
    );
}

/// Tune one per-session egress socket; a refusal is logged, never fatal.
fn configure_session_socket(socket: &UdpSocket, metrics: &ProxyMetrics) -> SocketTuningReport {
    let report = configure_socket(socket, SocketRole::Session, metrics);
    if report.failures > 0 {
        debug!(
            refusals = report.failures,
            "Session socket tuning degraded; using kernel defaults"
        );
    }
    report
}

/// How responses are written back to a client.
///
/// UDP sessions send raw datagrams to the client's bound address.  TCP
/// sessions write length-prefixed frames to the shared write half, guarded by
/// a [`CancellationToken`] so an in-flight send observes connection teardown.
#[derive(Clone)]
pub enum ClientSender {
    Udp {
        socket: Arc<UdpSocket>,
        addr: SocketAddrV4,
    },
    Tcp {
        write: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
        cancel: CancellationToken,
    },
}

impl std::fmt::Debug for ClientSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientSender::Udp { socket, addr } => f
                .debug_struct("Udp")
                .field("socket", socket)
                .field("addr", addr)
                .finish(),
            ClientSender::Tcp { cancel, .. } => f
                .debug_struct("Tcp")
                .field("cancelled", &cancel.is_cancelled())
                .finish(),
        }
    }
}

impl ClientSender {
    /// Send `bytes` to the client.  Returns the number of bytes written.
    pub async fn send(&self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            ClientSender::Udp { socket, addr } => socket.send_to(bytes, addr).await,
            ClientSender::Tcp { write, cancel } => {
                if cancel.is_cancelled() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "session cancelled",
                    ));
                }
                let mut guard = write.lock().await;
                write_frame(&mut *guard, bytes).await?;
                Ok(bytes.len())
            }
        }
    }

    /// Whether this sender writes to a TCP connection.
    pub fn is_tcp(&self) -> bool {
        matches!(self, ClientSender::Tcp { .. })
    }
}

/// Get current timestamp in microseconds (wrapping u32).
fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

/// Tracks an active client tunnel session.
#[derive(Debug)]
pub struct ClientSession {
    /// Client's address (for sending responses back).
    pub client_addr: SocketAddrV4,
    /// Game server this client is connected to.
    pub game_server: SocketAddrV4,
    /// Outbound socket for forwarding to game server.
    pub outbound_socket: Arc<UdpSocket>,
    /// How responses are sent back to the client (UDP or TCP).
    pub sender: ClientSender,
    /// Cancellation token, fired when the session is torn down so the
    /// response listener can observe the close and stop reading.
    pub cancel: CancellationToken,
    /// Packets relayed in this session.
    pub packets_relayed: u64,
    /// Bytes relayed in this session.
    pub bytes_relayed: u64,
    /// Session start time (monotonic epoch for all session durations).
    pub started_at: Instant,
    /// Microseconds since [`Self::started_at`] at the last activity, or 0 if
    /// the session has seen no traffic yet.  Written via [`Self::touch`].
    last_activity_us: AtomicU64,
    /// Microseconds since [`Self::started_at`] when the client packet currently
    /// awaiting a response was forwarded to the game server, or 0 when no
    /// forward is outstanding.  Written immediately before the upstream
    /// `send_to` for the normal and FEC-recovery paths, and cleared by the
    /// first response read on the session socket via `swap(0)`, so at most one
    /// latency sample is attributed per forward.  Refers to the most recent
    /// forward; a subsequent forward overwrites it.
    pub pending_forward_us: AtomicU64,
    /// Response sequence counter (FEC responses).
    pub response_seq: AtomicU16,
    /// Last client packet sequence seen (echoed in non-FEC responses so the
    /// client can dedupe multipath duplicate responses).
    pub last_client_seq: AtomicU16,
    /// Whether this client uses FEC (detected from first v2 packet).
    pub fec_enabled: bool,
    /// FEC block size (K) from the client's packets.
    pub fec_k: u8,
    /// FEC decoder for inbound packets (client → proxy).
    /// Protected by tokio Mutex since it's accessed from the inbound loop.
    pub fec_decoder: tokio::sync::Mutex<FecDecoder>,
    /// Idempotent claim flag for the response-listener task.
    ///
    /// `false` → no listener owns this session yet; `true` → one has been
    /// spawned.  Only the site that flips this from `false` to `true` may
    /// spawn [`run_session_response_listener`], which guarantees exactly one
    /// listener per session.
    listener_started: AtomicBool,
    /// Number of times [`ClientSession::claim_response_listener`] has won.
    /// Always 0 or 1; used by tests to prove single registration.
    listeners_spawned: AtomicU64,
    /// Successful forwards to the game server, for the relay-measured
    /// server-leg probe gate.
    ///
    /// Separate from [`Self::packets_relayed`] because that field is a handoff
    /// snapshot value and is never incremented on the hot path, while this one
    /// must be atomic and is read on every forward. Only once it reaches
    /// [`FORWARD_THRESHOLD`] may the relay probe this session's game server.
    forwarded_packets: AtomicU64,
    /// The data-plane session token whose server-leg interest is currently
    /// registered (0 = none). A token rotation changes it, and the forward path
    /// re-registers on change and periodically thereafter so the interest stays
    /// live without a lock on every packet.
    server_leg_token: AtomicU32,
}

impl ClientSession {
    /// Claim ownership of this session's response-listener task.
    ///
    /// Returns `true` for exactly one caller; every subsequent caller gets
    /// `false`.  Only the winning caller may spawn
    /// [`run_session_response_listener`] for this session.
    pub fn claim_response_listener(&self) -> bool {
        if self
            .listener_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.listeners_spawned.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Number of response listeners successfully claimed for this session.
    ///
    /// Always 0 or 1; exposed so tests can prove single registration.
    pub fn response_listeners_spawned(&self) -> u64 {
        self.listeners_spawned.load(Ordering::Relaxed)
    }

    /// Refresh this session's activity clock.
    ///
    /// Stores the current instant, as microseconds since [`Self::started_at`],
    /// into the lock-free activity field.  Safe to call concurrently from the
    /// inbound packet path and the response listener; a load racing a store on
    /// [`Ordering::Relaxed`] can only observe a slightly stale timestamp, which
    /// is harmless for an advisory liveness check.
    pub fn touch(&self) {
        self.last_activity_us.store(
            self.started_at.elapsed().as_micros() as u64,
            Ordering::Relaxed,
        );
    }

    /// Microseconds of idle time since this session's last activity.
    ///
    /// Computed in the monotonic [`Self::started_at`] clock as
    /// `now_micros - last_activity_us`.  A never-touched session reports its
    /// full age (the stored value is 0), and the subtraction saturates at zero
    /// so a timestamp that appears to be in the future can never underflow.
    pub fn idle_micros(&self) -> u64 {
        (self.started_at.elapsed().as_micros() as u64)
            .saturating_sub(self.last_activity_us.load(Ordering::Relaxed))
    }
}

/// Command sent from the inbound packet path to a TCP connection task. The
/// task buffers these until the upstream dial completes, then applies them in
/// arrival order.
enum TcpCmd {
    /// One batch of in-order game stream payloads to write to the real server.
    ///
    /// Batches are enqueued once per accepted inbound packet with `try_send`,
    /// so the inbound path never blocks on a stalled game server: the
    /// per-connection gate in [`process_tcp_packet`] refuses a segment instead
    /// of waiting for a slot.
    Data(Vec<bytes::Bytes>),
    /// The client finished sending; shut the write half toward the server.
    Fin,
}

/// Proxy-side connection state for one TCP-tunnelled game connection, keyed
/// by its 5-tuple (see [`Tcp5Tuple`]) and owned by the client that opened it.
///
/// The 5-tuple is carried verbatim in the tunnel header, so it is client-supplied
/// and not unique across the relay: two clients behind different NATs can present
/// the same game source and destination. The owner is therefore checked on every
/// packet; without it, any authenticated client could reset or inject into another
/// client's connection by guessing the tuple.
pub struct TcpConnState {
    /// The connection's 5-tuple (game source and destination).
    pub tuple: Tcp5Tuple,
    /// The authenticated client that created this connection.
    owner: SocketAddrV4,
    /// The shared sequenced-reliable channel for this connection. The receiver
    /// orders client→server bytes; the sender sequences server→client bytes.
    /// Mutex-protected because the inbound path and the connection task each
    /// hold one side.
    pub reliable: tokio::sync::Mutex<TcpReliable>,
    /// Outbound command channel toward the connection task. The connection
    /// task owns the receiver; this side is cloned for each send and kept for
    /// the life of the connection, so the channel only closes when the state
    /// is dropped.
    conn_tx: mpsc::Sender<TcpCmd>,
    /// Inbound command channel receiver, owned by the connection task.
    conn_rx: std::sync::Mutex<Option<mpsc::Receiver<TcpCmd>>>,
    /// Bytes accepted into the command queue but not yet written upstream.
    ///
    /// The inbound path reserves this budget before `on_segment` (so a refusal
    /// never advances the cumulative ACK) and the connection task releases it
    /// after each successful `write_all`. `Relaxed` is enough: the budget is
    /// advisory backpressure, and losing a race only costs one extra segment of
    /// queueing.
    queued_bytes: AtomicUsize,
    /// Cancellation token: fired on RST, idle expiry, or upstream teardown.
    cancel: CancellationToken,
    /// FEC decoder for inbound v5 packets on this connection. A block's data
    /// and parity packets converge here so recovery targets the right stream.
    pub fec_decoder: tokio::sync::Mutex<FecDecoder>,
    /// Last activity instant, for the idle sweep.
    last_activity: std::sync::Mutex<Instant>,
    /// FEC block size (K) the client uses on this leg, captured from the SYN.
    pub fec_k: u8,
    /// Set once the upstream dial completes; the task alone writes it.
    established: AtomicBool,
    /// Last client packet sequence seen (echoed in responses for dedup).
    pub last_client_seq: AtomicU16,
    /// Data packets accepted for forwarding to the game server, for the
    /// relay-measured server-leg probe gate.
    ///
    /// The TCP data path has no [`ClientSession`], so this is the connection's
    /// own count; it is otherwise the same gate as the UDP path's
    /// [`ClientSession::forwarded_packets`]. Only once it reaches
    /// [`FORWARD_THRESHOLD`] may the relay probe this connection's game server.
    forwarded_packets: AtomicU64,
    /// The data-plane session token whose server-leg interest is currently
    /// registered for this connection (0 = none). A token rotation changes it,
    /// and the forwarding path re-registers on change and periodically
    /// thereafter, exactly as the UDP path does.
    server_leg_token: AtomicU32,
}

impl TcpConnState {
    /// Create the state and its command channel. `fec_k` is the client leg's
    /// FEC block size; `cancel` is the shared teardown signal.
    pub fn new(
        tuple: Tcp5Tuple,
        owner: SocketAddrV4,
        fec_k: u8,
        cancel: CancellationToken,
    ) -> Self {
        let (tx, rx) = mpsc::channel(TCP_CMD_QUEUE_SLOTS);
        Self {
            tuple,
            owner,
            reliable: tokio::sync::Mutex::new(TcpReliable::new(2, 2)),
            conn_tx: tx,
            conn_rx: std::sync::Mutex::new(Some(rx)),
            queued_bytes: AtomicUsize::new(0),
            cancel,
            fec_decoder: tokio::sync::Mutex::new(FecDecoder::new()),
            last_activity: std::sync::Mutex::new(Instant::now()),
            fec_k,
            established: AtomicBool::new(false),
            last_client_seq: AtomicU16::new(0),
            forwarded_packets: AtomicU64::new(0),
            server_leg_token: AtomicU32::new(0),
        }
    }

    /// Take the command channel receiver, returning `None` once taken.
    fn take_conn_rx(&self) -> Option<mpsc::Receiver<TcpCmd>> {
        self.conn_rx.lock().unwrap().take()
    }

    /// Whether the command queue has a free slot for one more batch.
    ///
    /// Only [`process_tcp_packet`] sends `Data` on this channel and it runs one
    /// packet at a time per connection, so a free slot observed here is still
    /// free when the batch is enqueued a few instructions later (the only other
    /// sender, the FIN path, runs in the same task).
    fn has_command_capacity(&self) -> bool {
        self.conn_tx.capacity() > 0
    }

    /// Reserve `bytes` of this connection's queue byte budget.
    ///
    /// Returns `false` when the budget is exhausted. The caller must not touch
    /// the reliability receiver in that case: a segment the relay refuses must
    /// also be one the cumulative ACK does not claim, so the client resends it.
    fn try_reserve_queued_bytes(&self, bytes: usize) -> bool {
        self.queued_bytes
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |queued| {
                queued
                    .checked_add(bytes)
                    .filter(|next| *next <= TCP_CMD_QUEUE_BYTES)
            })
            .is_ok()
    }

    /// Release `bytes` of queue budget once the writer forwarded them upstream.
    fn release_queued_bytes(&self, bytes: usize) {
        self.queued_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// The authenticated client that opened this connection.
    pub fn owner(&self) -> SocketAddrV4 {
        self.owner
    }

    /// Refresh this connection's activity clock.
    pub fn touch(&self) {
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    /// Idle time since the last activity.
    pub fn idle(&self) -> Duration {
        self.last_activity.lock().unwrap().elapsed()
    }
}

/// Proxy-side geo aggregation context bound to a relay engine.
///
/// When present, a newly created session resolves both endpoints to country
/// codes and folds the pair into [`ProxyMetrics`]; lookup misses and
/// self-tunnels are counted in their own families. No raw IP is retained.
#[derive(Clone)]
pub struct GeoState {
    /// Resolves an IP to its two-letter country code.
    pub resolver: Arc<dyn GeoResolver>,
    /// Metrics collector the geo cells are recorded into.
    pub metrics: Arc<ProxyMetrics>,
    /// This relay's public IP, when known. Enables exact address-based
    /// self-tunnel detection; the port heuristic is the fallback.
    pub public_ip: Option<Ipv4Addr>,
    /// Control-plane port used by the port-based self-tunnel fallback.
    pub control_port: u16,
    /// Data-plane port used by the port-based self-tunnel fallback.
    pub data_port: u16,
}

/// The relay engine, manages all active tunnel sessions.
pub struct RelayEngine {
    /// Active client sessions indexed by client address.
    sessions: Arc<RwLock<HashMap<SocketAddrV4, Arc<ClientSession>>>>,
    /// Active TCP-tunnelled connections, keyed by their 5-tuple. Separate from
    /// [`Self::sessions`] (keyed by client address, bound to the UDP data
    /// plane) so the UDP session sweep never sees TCP keys.
    tcp_conns: Arc<RwLock<HashMap<Tcp5Tuple, Arc<TcpConnState>>>>,
    /// Maximum concurrent sessions.
    max_sessions: usize,
    /// Session timeout (no activity).
    session_timeout: Duration,
    /// Join handles for response-listener tasks spawned outside the manager.
    ///
    /// The immediate spawn in [`process_inbound_packet`] is otherwise detached,
    /// so its handle is parked here and the periodic manager aborts it once the
    /// session disappears.  Without this, an expired session (no FIN) would leak
    /// its listener task forever.  Manager-created handles stay in the manager's
    /// local `known_sessions`, which it already aborts.
    listener_handles: Arc<tokio::sync::Mutex<HashMap<SocketAddrV4, tokio::task::JoinHandle<()>>>>,
    /// Set while an in-place handoff is staged: new sessions are refused but
    /// existing sessions keep resolving.
    handoff_frozen: AtomicBool,
    /// Proxy-side geo aggregation, or `None` when geo is disabled.
    geo: Option<GeoState>,
    /// Sink for per-session socket-tuning observations. Defaults to a private
    /// collector so engine unit tests need no wiring; production attaches the
    /// process collector with [`RelayEngine::with_metrics`].
    socket_metrics: Arc<ProxyMetrics>,
    /// Optional egress budget guard. `None` (the default) disables the guard
    /// entirely, so a relay with no budget behaves exactly as before.
    budget: Option<Arc<BudgetGuard>>,
    /// Relay-measured server-leg prober, shared with the control plane.
    ///
    /// `None` (the default) means probing is disabled: no session registers
    /// interest, no probe is issued and the control plane never pushes a
    /// server-leg report. Production attaches the process registry with
    /// [`RelayEngine::with_server_leg`].
    server_leg: Option<Arc<ServerLegRegistry>>,
    /// Relay-side TTL traceroute, shared with the control plane.
    ///
    /// It rides the same data-path interest as [`Self::server_leg`]: a session
    /// that has forwarded [`FORWARD_THRESHOLD`] packets to a public game server
    /// also becomes a trace target, on a much slower cadence. `None` means no
    /// trace is ever issued.
    trace: Option<Arc<TraceRegistry>>,
}

impl RelayEngine {
    /// Create a new relay engine with the default 300 s session timeout.
    pub fn new(max_sessions: usize) -> Self {
        Self::new_with_timeout(max_sessions, Duration::from_secs(300)) // 5 min
    }

    /// Create a new relay engine with an explicit session timeout.
    ///
    /// Test seam: production code uses [`Self::new`] and its fixed 300 s
    /// timeout, while unit tests pass a short timeout so session expiry can be
    /// exercised without sleeping for minutes.
    pub fn new_with_timeout(max_sessions: usize, session_timeout: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            tcp_conns: Arc::new(RwLock::new(HashMap::new())),
            max_sessions,
            session_timeout,
            listener_handles: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            handoff_frozen: AtomicBool::new(false),
            geo: None,
            socket_metrics: Arc::new(ProxyMetrics::new()),
            budget: None,
            server_leg: None,
            trace: None,
        }
    }

    /// Attach proxy-side geo aggregation to this engine.
    pub fn with_geo(mut self, geo: GeoState) -> Self {
        self.geo = Some(geo);
        self
    }

    /// Attach the process metrics collector for socket-tuning observations.
    pub fn with_metrics(mut self, metrics: Arc<ProxyMetrics>) -> Self {
        self.socket_metrics = metrics;
        self
    }

    /// Attach the egress budget guard.
    ///
    /// Omitted by default; when absent the hard threshold is never enforced.
    pub fn with_budget(mut self, budget: Arc<BudgetGuard>) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Attach the relay-measured server-leg prober.
    ///
    /// Omitted by default; when absent no session ever registers probe interest.
    pub fn with_server_leg(mut self, registry: Arc<ServerLegRegistry>) -> Self {
        self.server_leg = Some(registry);
        self
    }

    /// Attach the relay-side TTL traceroute.
    ///
    /// Omitted by default; when absent no trace is ever issued. Production
    /// attaches it together with the server-leg registry under the same
    /// per-relay switch: both measurements target exactly the same set of
    /// already-forwarded destinations.
    pub fn with_trace(mut self, registry: Arc<TraceRegistry>) -> Self {
        self.trace = Some(registry);
        self
    }

    /// Tell the server-leg prober that `session` forwarded one more packet.
    ///
    /// Runs on the hot path, so it is two atomics and no lock in the common
    /// case: the registry is only touched once the session has crossed
    /// [`FORWARD_THRESHOLD`], when its token changes, and then once every
    /// [`SERVER_LEG_REFRESH_PACKETS`] forwards to keep the interest live.
    fn note_server_leg_forward(&self, session: &ClientSession, session_token: u32) {
        let Some(registry) = self.server_leg.as_ref() else {
            return;
        };
        // Relaxed is enough: the count only gates a probe and a lost increment
        // costs at most one packet of delay, never correctness.
        let forwarded = session
            .forwarded_packets
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.note_server_leg_progress(
            registry,
            forwarded,
            session_token,
            *session.client_addr.ip(),
            *session.game_server.ip(),
            &session.server_leg_token,
        );
    }

    /// Tell the server-leg prober that a TCP-tunnelled connection accepted one
    /// more data packet for its game server.
    ///
    /// This is the TCP analogue of [`Self::note_server_leg_forward`]. A TCP
    /// connection is tracked by [`TcpConnState`] rather than [`ClientSession`]
    /// (separate lifetimes, a byte stream instead of datagrams), but the
    /// reported interest is keyed by the same control-plane session token, so a
    /// report arrives on the client's control connection exactly as a UDP
    /// report does. Counting happens at the inbound v5 data path, where every
    /// accepted data packet passes; no restructuring was needed.
    fn note_server_leg_forward_tcp(&self, conn: &TcpConnState, session_token: u32) {
        let Some(registry) = self.server_leg.as_ref() else {
            return;
        };
        let forwarded = conn
            .forwarded_packets
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.note_server_leg_progress(
            registry,
            forwarded,
            session_token,
            *conn.owner().ip(),
            conn.tuple.dst_ip,
            &conn.server_leg_token,
        );
    }

    /// Register or refresh probe interest once a measured source has crossed
    /// [`FORWARD_THRESHOLD`] and its token changed or the refresh cadence came
    /// due. Shared by the UDP session path and the TCP connection path.
    fn note_server_leg_progress(
        &self,
        registry: &ServerLegRegistry,
        forwarded: u64,
        session_token: u32,
        client_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
        registered_token: &AtomicU32,
    ) {
        if forwarded < FORWARD_THRESHOLD {
            return;
        }
        let token_changed =
            registered_token.swap(session_token, Ordering::Relaxed) != session_token;
        if token_changed || forwarded.is_multiple_of(SERVER_LEG_REFRESH_PACKETS) {
            registry.note_forward(session_token, client_ip, server_ip, forwarded);
            // The TTL traceroute rides the same interest: same forwarded-only
            // target gate, same token, its own (much slower) cadence.
            if let Some(trace) = self.trace.as_ref() {
                trace.note_forward(session_token, client_ip, server_ip, forwarded);
            }
        }
    }

    /// Evaluate the egress budget against `metrics` and publish the result.
    ///
    /// Called periodically by the session manager so the soft threshold is
    /// logged and exported even when no new session is attempted. A no-op when
    /// no guard is attached.
    pub fn observe_budget(&self, metrics: &ProxyMetrics) {
        let Some(budget) = &self.budget else {
            return;
        };
        let used = budget.metered_total(metrics.bytes_relayed.load(Ordering::Relaxed));
        budget.observe(used, super::budget::current_month_key(), metrics);
    }

    /// Record a newly created session's country pair, when geo is enabled.
    ///
    /// A self-tunnel is counted and skipped, and a resolver miss on either
    /// side counts a miss instead of a pair. Called only from the single
    /// new-session insert site, so a session is recorded at most once.
    fn record_geo_session(&self, session: &ClientSession) {
        let Some(geo) = &self.geo else {
            return;
        };
        let dst = session.game_server.ip();
        let self_tunnel = match geo.public_ip {
            Some(public_ip) => *dst == public_ip,
            None => {
                // Fallback when [server] public_ip is unset: only the QUIC
                // control port is a strong self-tunnel signal. The data port
                // is deliberately excluded because game traffic can
                // legitimately use 4434, and dropping it would hide real
                // demand. Operators should set public_ip for an exact match.
                session.game_server.port() == geo.control_port
            }
        };
        if self_tunnel {
            geo.metrics.record_geo_self_tunnel_skip();
            return;
        }
        let src = session.client_addr.ip();
        let Some(src_country) = geo.resolver.country(*src) else {
            geo.metrics.record_geo_miss(GeoSide::Src);
            return;
        };
        let Some(dst_country) = geo.resolver.country(*dst) else {
            geo.metrics.record_geo_miss(GeoSide::Dst);
            return;
        };
        geo.metrics.record_geo_session(&src_country, &dst_country);
    }

    /// Refuse new session creation while an in-place handoff is prepared.
    ///
    /// The window is milliseconds, and existing sessions keep resolving.
    pub fn freeze_handoff(&self) {
        self.handoff_frozen.store(true, Ordering::Release);
    }

    /// Resume accepting new sessions after a handoff attempt did not exec.
    pub fn unfreeze_handoff(&self) {
        self.handoff_frozen.store(false, Ordering::Release);
    }

    /// Whether new-session creation is currently frozen for a handoff.
    pub fn is_handoff_frozen(&self) -> bool {
        self.handoff_frozen.load(Ordering::Acquire)
    }

    /// Get the number of active sessions.
    pub async fn active_sessions(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Check if we can accept a new session.
    pub async fn can_accept(&self) -> bool {
        self.sessions.read().await.len() < self.max_sessions
    }

    /// Get or create a session for a client.
    ///
    /// If the client already has an active session, returns `(session, false)`.
    /// Otherwise, creates a new outbound socket and session, returning `(session, true)`.
    /// The caller should spawn a response listener when `is_new` is true.
    pub async fn get_or_create_session(
        &self,
        client_addr: SocketAddrV4,
        game_server: SocketAddrV4,
        fec_enabled: bool,
        fec_k: u8,
        sender: ClientSender,
    ) -> anyhow::Result<(Arc<ClientSession>, bool)> {
        // Fast path: check existing session
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&client_addr) {
                return Ok((Arc::clone(session), false));
            }
        }

        // Slow path: create new session. A staged handoff admits no new
        // sessions, but the fast path above already served existing ones.
        if self.is_handoff_frozen() {
            anyhow::bail!("handoff in progress: refusing to create a new session");
        }
        // Egress budget: refuse only NEW sessions at the hard threshold. The
        // fast path above already returned any existing session, so live
        // traffic is never interrupted.
        if let Some(budget) = &self.budget {
            let used =
                budget.metered_total(self.socket_metrics.bytes_relayed.load(Ordering::Relaxed));
            budget
                .check_new_session(used, super::budget::current_month_key())
                .map_err(anyhow::Error::from)?;
        }
        if !self.can_accept().await {
            anyhow::bail!("Max sessions ({}) reached", self.max_sessions);
        }

        // Bind a new outbound socket for this client's game traffic
        let outbound_socket = UdpSocket::bind("0.0.0.0:0").await?;
        let tuning = configure_session_socket(&outbound_socket, &self.socket_metrics);

        // A TCP session shares its connection's cancellation token so the
        // response listener stops when the connection drops; UDP sessions get
        // an independent token.
        let cancel = match &sender {
            ClientSender::Udp { .. } => CancellationToken::new(),
            ClientSender::Tcp { cancel, .. } => cancel.clone(),
        };

        info!(
            client = %client_addr,
            game_server = %game_server,
            outbound_port = %outbound_socket.local_addr()?,
            rcvbuf_effective = ?tuning.rcvbuf_effective,
            sndbuf_effective = ?tuning.sndbuf_effective,
            fec = fec_enabled,
            "New client session created"
        );

        let session = Arc::new(ClientSession {
            client_addr,
            game_server,
            outbound_socket: Arc::new(outbound_socket),
            sender,
            cancel,
            packets_relayed: 0,
            bytes_relayed: 0,
            started_at: Instant::now(),
            last_activity_us: AtomicU64::new(0),
            pending_forward_us: AtomicU64::new(0),
            response_seq: AtomicU16::new(0),
            last_client_seq: AtomicU16::new(0),
            fec_enabled,
            fec_k,
            fec_decoder: tokio::sync::Mutex::new(FecDecoder::new()),
            listener_started: AtomicBool::new(false),
            listeners_spawned: AtomicU64::new(0),
            forwarded_packets: AtomicU64::new(0),
            server_leg_token: AtomicU32::new(0),
        });

        let mut sessions = self.sessions.write().await;
        // Re-check the freeze under the write lock: a request that passed the
        // earlier check but stalled must not insert a session after the handoff
        // snapshot was taken. Re-check for a concurrent creator so this insert
        // cannot clobber an existing session.
        if self.is_handoff_frozen() {
            anyhow::bail!("handoff in progress: refusing to create a new session");
        }
        if let Some(existing) = sessions.get(&client_addr) {
            return Ok((Arc::clone(existing), false));
        }
        sessions.insert(client_addr, Arc::clone(&session));
        drop(sessions);
        self.record_geo_session(&session);

        Ok((session, true))
    }

    /// Get a shared reference to the sessions map (for the response listener).
    pub fn sessions(&self) -> Arc<RwLock<HashMap<SocketAddrV4, Arc<ClientSession>>>> {
        Arc::clone(&self.sessions)
    }

    /// Clean up expired sessions.
    ///
    /// A session expires once its idle time (see [`ClientSession::idle_micros`])
    /// reaches the configured timeout.  Dropping a session cancels its token so
    /// any response-listener task parked on it exits promptly.
    pub async fn cleanup_expired(&self) -> usize {
        let timeout = self.session_timeout;
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|addr, session| {
            let keep = session.idle_micros() < timeout.as_micros() as u64;
            if !keep {
                session.cancel.cancel();
                info!(client = %addr, "Session expired after {:?}", session.started_at.elapsed());
            }
            keep
        });
        before - sessions.len()
    }

    /// Park a response-listener join handle so the session manager can abort
    /// it after the session is removed.
    ///
    /// If a stale handle already exists for `addr` (client address reused after
    /// an expiry that fired no cancellation), it is aborted first so it cannot
    /// outlive its session.
    pub async fn register_listener_handle(
        &self,
        addr: SocketAddrV4,
        handle: tokio::task::JoinHandle<()>,
    ) {
        let mut handles = self.listener_handles.lock().await;
        if let Some(previous) = handles.insert(addr, handle) {
            previous.abort();
        }
    }

    /// Abort and forget every parked listener handle whose session is no longer
    /// active.
    pub async fn reap_listener_handles(&self, active: &HashMap<SocketAddrV4, Arc<ClientSession>>) {
        let mut handles = self.listener_handles.lock().await;
        handles.retain(|addr, handle| {
            if active.contains_key(addr) {
                true
            } else {
                handle.abort();
                false
            }
        });
    }

    /// Number of active TCP-tunnelled connections.
    pub async fn tcp_connection_count(&self) -> usize {
        self.tcp_conns.read().await.len()
    }

    /// Get or create the TCP connection for a 5-tuple owned by `client_addr`.
    ///
    /// Creation counts like one session: it obeys `max_sessions` (against the
    /// UDP session count plus the TCP connection count) and the egress budget
    /// hard threshold. Returns `(state, true)` on creation and
    /// `(state, false)` when the connection already existed.
    ///
    /// A tuple that already belongs to a different client is refused: the
    /// 5-tuple is client-supplied, so returning the existing state would let one
    /// client splice into another's stream.
    pub async fn get_or_create_tcp_connection(
        &self,
        tuple: Tcp5Tuple,
        client_addr: SocketAddrV4,
        fec_k: u8,
    ) -> anyhow::Result<(Arc<TcpConnState>, bool)> {
        {
            let conns = self.tcp_conns.read().await;
            if let Some(existing) = conns.get(&tuple) {
                if existing.owner() != client_addr {
                    anyhow::bail!("TCP 5-tuple is owned by another client");
                }
                return Ok((Arc::clone(existing), false));
            }
        }

        if self.is_handoff_frozen() {
            anyhow::bail!("handoff in progress: refusing to create a TCP connection");
        }
        if let Some(budget) = &self.budget {
            let used =
                budget.metered_total(self.socket_metrics.bytes_relayed.load(Ordering::Relaxed));
            budget
                .check_new_session(used, super::budget::current_month_key())
                .map_err(anyhow::Error::from)?;
        }

        let conn = Arc::new(TcpConnState::new(
            tuple,
            client_addr,
            fec_k,
            CancellationToken::new(),
        ));
        let mut conns = self.tcp_conns.write().await;
        if self.is_handoff_frozen() {
            anyhow::bail!("handoff in progress: refusing to create a TCP connection");
        }
        if let Some(existing) = conns.get(&tuple) {
            if existing.owner() != client_addr {
                anyhow::bail!("TCP 5-tuple is owned by another client");
            }
            return Ok((Arc::clone(existing), false));
        }
        // The shared session budget: one UDP session plus one TCP connection
        // must not exceed `max_sessions`.
        let total = self.sessions.read().await.len() + conns.len();
        if total >= self.max_sessions {
            anyhow::bail!("Max sessions ({}) reached", self.max_sessions);
        }
        conns.insert(tuple, Arc::clone(&conn));
        Ok((conn, true))
    }

    /// Remove a TCP connection from the table, cancelling its task.
    pub async fn remove_tcp_connection(&self, tuple: &Tcp5Tuple) -> Option<Arc<TcpConnState>> {
        let conn = self.tcp_conns.write().await.remove(tuple);
        if let Some(conn) = &conn {
            conn.cancel.cancel();
        }
        conn
    }

    /// Access the TCP connection table (for the connection tasks' teardown).
    fn tcp_table(&self) -> Arc<RwLock<HashMap<Tcp5Tuple, Arc<TcpConnState>>>> {
        Arc::clone(&self.tcp_conns)
    }

    /// Sweep TCP connections idle past the session timeout: RST both sides by
    /// cancelling the connection task and dropping the entry. Returns how many
    /// were removed.
    pub async fn cleanup_expired_tcp(&self) -> usize {
        let timeout = self.session_timeout;
        let expired: Vec<Tcp5Tuple> = {
            let conns = self.tcp_conns.read().await;
            conns
                .iter()
                .filter_map(|(tuple, conn)| (conn.idle() >= timeout).then_some(*tuple))
                .collect()
        };
        let removed = expired.len();
        for tuple in expired {
            if let Some(conn) = self.remove_tcp_connection(&tuple).await {
                info!(
                    src = %conn.tuple.src_ip,
                    src_port = conn.tuple.src_port,
                    dst = %conn.tuple.dst_ip,
                    dst_port = conn.tuple.dst_port,
                    "TCP connection expired after {:?}",
                    conn.idle()
                );
            }
        }
        removed
    }

    /// Snapshot every UDP session for an in-place handoff.
    ///
    /// FEC decoder state and `pending_forward_us` are intentionally not carried:
    /// the decoder is rebuilt by the adopting process and the latency marker is
    /// zeroed so no post-handoff response is charged a bogus lag.  TCP sessions
    /// cannot survive an `execve` (their write half lives in this process) and
    /// are skipped.
    #[cfg(target_os = "linux")]
    pub fn snapshot_handoff(&self, now: Instant) -> Vec<SessionSnapshot> {
        use std::os::fd::AsRawFd;

        let sessions = self.sessions.blocking_read();
        let mut snapshots = Vec::with_capacity(sessions.len());
        let mut tcp_skipped = 0usize;
        for session in sessions.values() {
            if session.sender.is_tcp() {
                tcp_skipped += 1;
                continue;
            }
            let age_us = now
                .saturating_duration_since(session.started_at)
                .as_micros() as u64;
            let last_activity_us = session.last_activity_us.load(Ordering::Relaxed);
            snapshots.push(SessionSnapshot {
                client_addr: session.client_addr.to_string(),
                game_server: session.game_server.to_string(),
                outbound_fd: session.outbound_socket.as_raw_fd(),
                fec_enabled: session.fec_enabled,
                fec_k: session.fec_k,
                age_us,
                idle_us: age_us.saturating_sub(last_activity_us),
                response_seq: session.response_seq.load(Ordering::Relaxed),
                last_client_seq: session.last_client_seq.load(Ordering::Relaxed),
                packets_relayed: session.packets_relayed,
                bytes_relayed: session.bytes_relayed,
            });
        }
        drop(sessions);
        if tcp_skipped > 0 {
            info!(
                tcp_sessions_skipped = tcp_skipped,
                "Handoff snapshot skipped TCP sessions"
            );
        }
        snapshots
    }

    /// Adopt snapshotted sessions into this engine.
    ///
    /// Each snapshot's outbound fd is validated and adopted into a fresh
    /// non-blocking socket, a new [`ClientSession`] is built (new FEC decoder,
    /// new cancellation token, re-anchored age, preserved activity and
    /// counters), and its response listener is spawned immediately.  An address
    /// that already has a session, or whose fd fails validation, is skipped.
    /// Returns the number of sessions installed.
    #[cfg(target_os = "linux")]
    pub async fn install_handoff_sessions(
        &self,
        snaps: &[SessionSnapshot],
        data_socket: Arc<UdpSocket>,
        metrics: Arc<ProxyMetrics>,
    ) -> usize {
        let mut installed = 0usize;
        for snap in snaps {
            let client_addr: SocketAddrV4 = match snap.client_addr.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    warn!(
                        client = %snap.client_addr,
                        "Skipping handoff session with unparseable client_addr"
                    );
                    crate::handoff::close_fd(snap.outbound_fd);
                    continue;
                }
            };
            let game_server: SocketAddrV4 = match snap.game_server.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    warn!(
                        client = %snap.client_addr,
                        "Skipping handoff session with unparseable game_server"
                    );
                    crate::handoff::close_fd(snap.outbound_fd);
                    continue;
                }
            };

            {
                let sessions = self.sessions.read().await;
                if sessions.contains_key(&client_addr) {
                    debug!(
                        client = %client_addr,
                        "Handoff session address already active, skipping"
                    );
                    drop(sessions);
                    crate::handoff::close_fd(snap.outbound_fd);
                    continue;
                }
            }

            let std_socket = match crate::handoff::adopt_std_udp(snap.outbound_fd) {
                Ok(socket) => socket,
                Err(e) => {
                    warn!(
                        client = %client_addr,
                        fd = snap.outbound_fd,
                        error = %e,
                        "Skipping handoff session with invalid outbound fd"
                    );
                    continue;
                }
            };
            let outbound_socket = match UdpSocket::from_std(std_socket) {
                Ok(socket) => socket,
                Err(e) => {
                    warn!(
                        client = %client_addr,
                        error = %e,
                        "Skipping handoff session: cannot register adopted socket"
                    );
                    continue;
                }
            };
            configure_session_socket(&outbound_socket, &metrics);

            // Seal the adopted fd here, where ownership is established, so the
            // caller never touches fds of snapshots that were skipped/closed.
            let _ = crate::handoff::set_cloexec(snap.outbound_fd);

            let age = Duration::from_micros(snap.age_us);
            let started_at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
            let session = Arc::new(ClientSession {
                client_addr,
                game_server,
                outbound_socket: Arc::new(outbound_socket),
                sender: ClientSender::Udp {
                    socket: Arc::clone(&data_socket),
                    addr: client_addr,
                },
                cancel: CancellationToken::new(),
                packets_relayed: snap.packets_relayed,
                bytes_relayed: snap.bytes_relayed,
                started_at,
                last_activity_us: AtomicU64::new(snap.age_us.saturating_sub(snap.idle_us)),
                pending_forward_us: AtomicU64::new(0),
                response_seq: AtomicU16::new(snap.response_seq),
                last_client_seq: AtomicU16::new(snap.last_client_seq),
                fec_enabled: snap.fec_enabled,
                fec_k: snap.fec_k,
                fec_decoder: tokio::sync::Mutex::new(FecDecoder::new()),
                listener_started: AtomicBool::new(false),
                listeners_spawned: AtomicU64::new(0),
                forwarded_packets: AtomicU64::new(0),
                server_leg_token: AtomicU32::new(0),
            });

            {
                let mut sessions = self.sessions.write().await;
                if sessions.contains_key(&client_addr) {
                    continue;
                }
                sessions.insert(client_addr, Arc::clone(&session));
            }

            metrics.record_session_created();
            if session.claim_response_listener() {
                let session_clone = Arc::clone(&session);
                let metrics_clone = Arc::clone(&metrics);
                let server_leg = self.server_leg.clone();
                let handle = tokio::spawn(async move {
                    run_session_response_listener(session_clone, metrics_clone, server_leg).await;
                });
                self.register_listener_handle(client_addr, handle).await;
            }
            installed += 1;
        }
        installed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Linux recvmmsg batch state
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum packets to drain in one `recvmmsg` syscall (Linux only).
#[cfg(target_os = "linux")]
const BATCH: usize = 32;

/// Per-slot receive-buffer size.
#[cfg(target_os = "linux")]
const BUF_SIZE: usize = 2048;

/// Heap-allocated kernel-facing state for one `recvmmsg` call.
///
/// `bufs`, raw receive buffers (32 × 2048 B = 64 KiB, on the task heap).
/// `msgs`, `mmsghdr` array handed to the kernel.
/// `saddrs`, `sockaddr_in` source-address slots, one per message.
///
/// `iovec` entries are rebuilt inside [`BatchState::do_recv`] on every call so
/// the struct is not self-referential and does not require `Pin`.
#[cfg(target_os = "linux")]
struct BatchState {
    bufs: [[u8; BUF_SIZE]; BATCH],
    msgs: [libc::mmsghdr; BATCH],
    saddrs: [libc::sockaddr_in; BATCH],
}

// SAFETY: `BatchState` is only ever accessed from one Tokio task at a time.
// The raw pointer fields inside `libc::mmsghdr` are implementation details of
// the recvmmsg call; they are re-initialised on every `do_recv` invocation and
// are never observed across an async suspension point.
#[cfg(target_os = "linux")]
unsafe impl Send for BatchState {}

#[cfg(target_os = "linux")]
impl BatchState {
    fn new() -> Self {
        // SAFETY: zero-initialising C POD structs is always valid.
        unsafe {
            Self {
                bufs: [[0u8; BUF_SIZE]; BATCH],
                msgs: std::mem::zeroed(),
                saddrs: std::mem::zeroed(),
            }
        }
    }

    /// Non-blocking `recvmmsg` call.
    ///
    /// Rebuilds iovecs on the stack before every call so the struct is not
    /// self-referential.  Returns `Ok(n)` where `n >= 1`, or
    /// `Err(WouldBlock)` when the socket has no pending data.
    fn do_recv(&mut self, fd: libc::c_int) -> std::io::Result<usize> {
        // Build iovecs on the stack.  They only need to live during this call.
        let mut iovecs: [libc::iovec; BATCH] =
            // SAFETY: zero-init is valid for iovec.
            unsafe { std::mem::zeroed() };

        // iovecs[i] and saddrs[i] live for the full duration of this function
        // (and therefore across the recvmmsg call below).
        #[allow(clippy::needless_range_loop)]
        for i in 0..BATCH {
            iovecs[i].iov_base = self.bufs[i].as_mut_ptr() as *mut libc::c_void;
            iovecs[i].iov_len = BUF_SIZE;

            let hdr = &mut self.msgs[i].msg_hdr;
            hdr.msg_iov = &mut iovecs[i] as *mut libc::iovec;
            hdr.msg_iovlen = 1;
            hdr.msg_name = &mut self.saddrs[i] as *mut libc::sockaddr_in as *mut libc::c_void;
            hdr.msg_namelen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            hdr.msg_control = std::ptr::null_mut();
            hdr.msg_controllen = 0;
            hdr.msg_flags = 0;

            self.msgs[i].msg_len = 0;
        }

        // SAFETY: `fd` is valid (live UdpSocket), arrays fully initialised above.
        let n = unsafe {
            libc::recvmmsg(
                fd,
                self.msgs.as_mut_ptr(),
                BATCH as libc::c_uint,
                libc::MSG_DONTWAIT,
                std::ptr::null_mut(),
            )
        };

        if n < 0 {
            // Includes EAGAIN → mapped to WouldBlock by the OS abstraction layer.
            return Err(std::io::Error::last_os_error());
        }
        Ok(n as usize)
    }
}

/// Wait for `sock` to become readable, then drain up to `BATCH` datagrams in
/// one `recvmmsg` syscall.
///
/// Uses `try_io` to properly arm/disarm Tokio's epoll interest:
/// - On success the readiness flag stays set (there may be more data).
/// - On `WouldBlock` the flag is cleared so `readable()` actually waits.
#[cfg(target_os = "linux")]
async fn recv_batch_async(sock: &UdpSocket, batch: &mut BatchState) -> std::io::Result<usize> {
    use std::os::unix::io::AsRawFd;
    let fd = sock.as_raw_fd();
    loop {
        // Block until epoll says data is available.
        sock.readable().await?;
        // try_io is synchronous, no Send requirement on the closure.
        match sock.try_io(tokio::io::Interest::READABLE, || batch.do_recv(fd)) {
            Ok(n) if n > 0 => return Ok(n),
            Ok(_) => continue, // shouldn't happen, re-arm just in case
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // try_io already cleared the readiness bit; loop to readable().
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Per-packet inbound processing (shared between all platforms)
// ─────────────────────────────────────────────────────────────────────────────

/// Process a single received inbound packet.
///
/// Contains the full hot-path per-packet logic: rate-limit, header decode,
/// auth, keepalive/FIN handling, abuse check, FEC, session management, and
/// game-server forwarding.  Extracted so it can be called from both the
/// Linux batched receive loop and the non-Linux single-recv loop.
#[allow(clippy::too_many_arguments)]
async fn process_inbound_packet(
    buf: &[u8],
    client_addr: SocketAddrV4,
    sender: &ClientSender,
    engine: &Arc<RelayEngine>,
    rate_limiter: &Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: &Arc<RwLock<Authenticator>>,
    abuse_detector: &Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: &Arc<ProxyMetrics>,
) -> bool {
    let len = buf.len();

    // ── Rate limit check ────────────────────────────────────────
    {
        let mut rl = rate_limiter.lock().await;
        match rl.check(client_addr, len as u64) {
            RateLimitResult::Allowed => {}
            RateLimitResult::PacketRateExceeded => {
                trace!(client = %client_addr, "Rate limited (PPS)");
                metrics.record_drop(DropReason::RateLimit);
                return false;
            }
            RateLimitResult::BandwidthExceeded => {
                trace!(client = %client_addr, "Rate limited (BPS)");
                metrics.record_drop(DropReason::RateLimit);
                return false;
            }
            RateLimitResult::IpPacketRateExceeded => {
                trace!(client = %client_addr, "Rate limited (per-IP PPS)");
                metrics.record_drop(DropReason::RateLimit);
                metrics.record_rate_limit_ip();
                return false;
            }
            RateLimitResult::IpBandwidthExceeded => {
                trace!(client = %client_addr, "Rate limited (per-IP BPS)");
                metrics.record_drop(DropReason::RateLimit);
                metrics.record_rate_limit_ip();
                return false;
            }
            RateLimitResult::IpTableFull => {
                trace!(client = %client_addr, "Rate limit table full");
                metrics.record_drop(DropReason::RateLimit);
                metrics.record_rate_limit_overflow();
                return false;
            }
            RateLimitResult::FlowTableFull => {
                trace!(client = %client_addr, "Rate limit flow table full");
                metrics.record_drop(DropReason::RateLimit);
                metrics.record_rate_limit_overflow();
                return false;
            }
        }
    }

    // ── Decode tunnel header ────────────────────────────────────
    let (header, payload) = match TunnelHeader::decode_with_payload(buf) {
        Ok(result) => result,
        Err(e) => {
            debug!(client = %client_addr, error = %e, "Invalid tunnel packet");
            metrics.record_drop(DropReason::Malformed);
            return false;
        }
    };

    // ── Control packets ─────────────────────────────────────────
    // Keepalives are answered without auth: they are "are you alive" pings the
    // client uses to probe latency before registering, and they reveal nothing
    // the public health endpoint does not already expose. (No amplification
    // risk, the response is the same size as the request.)
    if header.is_keepalive() {
        trace!(client = %client_addr, seq = header.sequence, "Keepalive received");
        let response = TunnelHeader::keepalive(header.sequence, now_us())
            .with_session_token(header.session_token);
        let response_bytes = response.encode_to_array();
        let _ = sender.send(&response_bytes).await;
        return false;
    }

    // ── Security: Authentication check ──────────────────────────
    {
        let auth = authenticator.read().await;
        if !auth.validate(
            *client_addr.ip(),
            client_addr.port(),
            header.session_token,
            Instant::now(),
        ) {
            debug!(
                client = %client_addr,
                token = header.session_token,
                "Unauthorized: invalid principal, port, or session token"
            );
            metrics.record_drop(DropReason::Auth);
            return false;
        }
    }

    if header.is_fin() {
        info!(client = %client_addr, "Client sent FIN, closing session");
        let sessions_lock = engine.sessions();
        let mut sessions = sessions_lock.write().await;
        if let Some(session) = sessions.remove(&client_addr) {
            session.cancel.cancel();
        }
        return true;
    }

    // Get the original destination (game server) from the header
    let game_server = header.orig_dst_addr();
    let is_fec = header.has_fec();

    // ── Security: Abuse detection (includes destination validation) ──
    {
        let mut abuse = abuse_detector.lock().await;
        match abuse.record_inbound(*client_addr.ip(), game_server, len as u64) {
            AbuseCheckResult::Allowed => {}
            AbuseCheckResult::PrivateDestination => {
                debug!(
                    client = %client_addr,
                    dest = %game_server,
                    "Blocked: private/internal destination"
                );
                metrics.record_drop(DropReason::Abuse);
                return false;
            }
            AbuseCheckResult::DestinationNotAllowed => {
                debug!(
                    client = %client_addr,
                    dest = %game_server,
                    "Blocked: destination not in allowlist"
                );
                metrics.record_drop(DropReason::Abuse);
                return false;
            }
            AbuseCheckResult::Banned => {
                trace!(client = %client_addr, "Blocked: client is banned");
                metrics.record_drop(DropReason::Abuse);
                return false;
            }
            AbuseCheckResult::ReflectionDetected => {
                warn!(client = %client_addr, "Blocked: reflection attack detected");
                metrics.record_drop(DropReason::Abuse);
                return false;
            }
            AbuseCheckResult::AmplificationDetected => {
                warn!(client = %client_addr, "Blocked: amplification detected");
                metrics.record_drop(DropReason::Abuse);
                return false;
            }
        }
    }

    // ── FEC: Parse FEC header if version 2 ──────────────────────
    let (fec_hdr, game_payload) = if is_fec {
        if payload.len() < FEC_HEADER_SIZE {
            debug!(client = %client_addr, "FEC packet too short");
            metrics.record_drop(DropReason::FecMalformed);
            return false;
        }
        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
        match FecHeader::decode(&mut fec_slice) {
            Some(fh) => (Some(fh), &payload[FEC_HEADER_SIZE..]),
            None => {
                debug!(client = %client_addr, "Invalid FEC header");
                metrics.record_drop(DropReason::FecMalformed);
                return false;
            }
        }
    } else {
        (None, payload)
    };

    // Determine FEC k_size for session creation
    let fec_k = fec_hdr.as_ref().map(|h| h.k_size).unwrap_or(4);

    // ── TCP-tunnelled connection (protocol v5) ───────────────────
    //
    // The keepalive → auth → FIN → abuse checks above already ran on the shared
    // tunnel header, so v5 inherits auth, rate limiting, and destination
    // validation for free. FEC is decoded identically before this branch; the
    // per-connection FecDecoder converges a block's parity and data packets.
    if header.has_tcp() {
        process_tcp_packet(
            &header,
            fec_hdr.as_ref(),
            game_payload,
            client_addr,
            sender,
            engine,
            metrics,
        )
        .await;
        return false;
    }

    // ── Session: get or create ───────────────────────────────────
    let (session, is_new) = match engine
        .get_or_create_session(client_addr, game_server, is_fec, fec_k, sender.clone())
        .await
    {
        Ok(s) => s,
        Err(e) => {
            if let Some(exceeded) = e.downcast_ref::<BudgetExceeded>() {
                warn!(
                    client = %client_addr,
                    limit_bytes = exceeded.limit,
                    used_bytes = exceeded.used,
                    used_pct = exceeded.pct,
                    "Egress budget reached: refusing new session (existing sessions continue)"
                );
                metrics.record_drop(DropReason::EgressBudget);
            } else {
                warn!(client = %client_addr, error = %e, "Failed to create session");
                metrics.record_drop(DropReason::SessionSetup);
            }
            return false;
        }
    };

    // Immediately spawn the response listener for a brand-new session. The
    // claim is idempotent, so if the periodic manager won the race this site
    // must not spawn a second listener. We park our handle in the engine
    // registry (rather than dropping it) so the manager can abort it when the
    // session is later removed.
    if is_new {
        metrics.record_session_created();
        if session.claim_response_listener() {
            let session_clone = Arc::clone(&session);
            let metrics_clone = Arc::clone(metrics);
            let server_leg = engine.server_leg.clone();
            let handle = tokio::spawn(async move {
                run_session_response_listener(session_clone, metrics_clone, server_leg).await;
            });
            engine.register_listener_handle(client_addr, handle).await;
            info!(client = %client_addr, fec = is_fec, "Response listener spawned immediately");
        }
    }

    // ── Handle FEC parity packets ───────────────────────────────
    if let Some(ref fh) = fec_hdr {
        if fh.is_parity() {
            // Parity packet: do NOT forward to game server.
            // Feed into FEC decoder for potential recovery.
            metrics.record_fec_parity();
            let parity_data = bytes::Bytes::copy_from_slice(game_payload);
            let mut decoder = session.fec_decoder.lock().await;
            if let Some((_idx, recovered)) = decoder.receive_parity(fh, parity_data) {
                // Recovered a lost packet, forward to game server
                metrics.record_fec_recovery();
                info!(
                    client = %client_addr,
                    block = fh.block_id,
                    recovered_len = recovered.len(),
                    "🔧 FEC recovered lost packet on proxy"
                );
                session.pending_forward_us.store(
                    session.started_at.elapsed().as_micros() as u64,
                    Ordering::Relaxed,
                );
                match session
                    .outbound_socket
                    .send_to(&recovered, game_server)
                    .await
                {
                    Ok(sent) => {
                        metrics.record_relay(sent as u64);
                    }
                    Err(e) => {
                        session.pending_forward_us.store(0, Ordering::Relaxed);
                        metrics.record_drop(DropReason::RelaySendError);
                        debug!(client = %client_addr, error = %e, "Failed to forward recovered packet");
                    }
                }
            }
            // Periodic GC
            decoder.gc();
            return false; // Don't forward parity to game server
        } else {
            // Data packet with FEC: track in decoder, then forward game_payload
            metrics.record_fec_data();
            let data_bytes = bytes::Bytes::copy_from_slice(game_payload);
            let mut decoder = session.fec_decoder.lock().await;
            decoder.receive_data(fh, data_bytes);
        }
    }

    // ── Forward the raw game payload to the game server ─────────
    // Stamp the monotonic send time before the await so the response listener
    // can measure the upstream lag when the matching response arrives.
    session.pending_forward_us.store(
        session.started_at.elapsed().as_micros() as u64,
        Ordering::Relaxed,
    );
    match session
        .outbound_socket
        .send_to(game_payload, game_server)
        .await
    {
        Ok(sent) => {
            metrics.record_relay(sent as u64);
            trace!(
                client = %client_addr,
                game_server = %game_server,
                seq = header.sequence,
                payload_len = game_payload.len(),
                fec = is_fec,
                "Forwarded to game server"
            );

            session
                .last_client_seq
                .store(header.sequence, Ordering::Relaxed);
            session.touch();
            // Relay-measured server leg: once this session has forwarded
            // enough to its game server, register probe interest for that
            // address. The prober only ever learns targets from here.
            engine.note_server_leg_forward(&session, header.session_token);

            let mut abuse = abuse_detector.lock().await;
            abuse.record_outbound(*client_addr.ip(), sent as u64);
        }
        Err(e) => {
            session.pending_forward_us.store(0, Ordering::Relaxed);
            debug!(
                client = %client_addr,
                game_server = %game_server,
                error = %e,
                "Failed to forward to game server"
            );
            metrics.record_drop(DropReason::RelaySendError);
        }
    }

    false
}

// ─────────────────────────────────────────────────────────────────────────────
//  TCP-tunnelled connection handling (protocol v5)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a v5 data response for the client: `[TunnelHeader][FecHeader]
/// [TcpExtHeader][payload]` with the source/destination swapped back to the
/// client, the connection's FEC header, and the reliability layer's seq/ack.
fn build_tcp_response_packet(
    client_header: &TunnelHeader,
    seq: u16,
    fec_hdr: &FecHeader,
    tcp_seq: u32,
    tcp_ack: u32,
    flags: u8,
    payload: &[u8],
) -> bytes::BytesMut {
    let response_header = client_header
        .make_response(seq, now_us())
        .with_session_token(client_header.session_token);
    let tcp_ext = TcpExtHeader {
        tcp_seq,
        tcp_ack,
        flags,
        window: 0,
        reserved: [0, 0],
    };
    build_tcp_packet(&response_header, fec_hdr, &tcp_ext, payload)
}

/// Build a v5 RST packet (zero payload, RST flag) to tear the client's
/// terminator down.
fn build_tcp_rst_packet(client_header: &TunnelHeader, fec_k: u8) -> bytes::BytesMut {
    let response_header = client_header.make_response(0, now_us());
    let fec_hdr = FecHeader::data(0, 0, fec_k.max(2));
    let tcp_ext = TcpExtHeader {
        tcp_seq: 0,
        tcp_ack: 0,
        flags: tcp_flags::RST,
        window: 0,
        reserved: [0, 0],
    };
    build_tcp_packet(&response_header, &fec_hdr, &tcp_ext, &[])
}

/// Route one v5 packet (already past auth/abuse and FEC framing) to its
/// per-5-tuple connection. This is the relay-side connector: on SYN it dials
/// the real server, on data it splices bytes upstream, on FIN it shuts the
/// write half, and on RST it tears the connection down.
#[allow(clippy::too_many_arguments)]
async fn process_tcp_packet(
    client_header: &TunnelHeader,
    fec_hdr: Option<&FecHeader>,
    game_payload: &[u8],
    client_addr: SocketAddrV4,
    sender: &ClientSender,
    engine: &Arc<RelayEngine>,
    metrics: &Arc<ProxyMetrics>,
) {
    let tuple = Tcp5Tuple {
        src_ip: client_header.orig_src_ip,
        src_port: client_header.orig_src_port,
        dst_ip: client_header.orig_dst_ip,
        dst_port: client_header.orig_dst_port,
    };

    let fec_k = fec_hdr.map(|h| h.k_size).unwrap_or(4).max(2);

    // SYN opens the connection; anything else needs an existing entry.
    let is_syn = decode_tcp_payload(game_payload)
        .map(|(ext, _)| ext.flags & tcp_flags::SYN != 0)
        .unwrap_or(false);
    let conn = if is_syn {
        match engine
            .get_or_create_tcp_connection(tuple, client_addr, fec_k)
            .await
        {
            Ok((conn, is_new)) => {
                if is_new {
                    metrics.record_session_created();
                    let engine = Arc::clone(engine);
                    let metrics = Arc::clone(metrics);
                    let sender = sender.clone();
                    let header = client_header.clone();
                    let conn_clone = Arc::clone(&conn);
                    tokio::spawn(async move {
                        run_tcp_connection(
                            conn_clone,
                            client_addr,
                            sender,
                            header,
                            engine,
                            metrics,
                        )
                        .await;
                    });
                    info!(
                        src = %tuple.src_ip,
                        src_port = tuple.src_port,
                        dst = %tuple.dst_ip,
                        dst_port = tuple.dst_port,
                        "TCP connection created"
                    );
                }
                conn
            }
            Err(e) => {
                if let Some(exceeded) = e.downcast_ref::<BudgetExceeded>() {
                    warn!(
                        client = %client_addr,
                        limit_bytes = exceeded.limit,
                        used_bytes = exceeded.used,
                        "Egress budget reached: refusing TCP connection"
                    );
                    metrics.record_drop(DropReason::EgressBudget);
                } else {
                    warn!(client = %client_addr, error = %e, "Failed to create TCP connection");
                    metrics.record_drop(DropReason::SessionSetup);
                }
                let _ = sender
                    .send(&build_tcp_rst_packet(client_header, fec_k))
                    .await;
                return;
            }
        }
    } else {
        let Some(conn) = engine.tcp_table().read().await.get(&tuple).cloned() else {
            trace!(
                client = %client_addr,
                "TCP packet for unknown 5-tuple dropped"
            );
            metrics.record_drop(DropReason::Malformed);
            return;
        };
        if conn.owner() != client_addr {
            warn!(
                client = %client_addr,
                owner = %conn.owner(),
                "TCP packet for another client's connection dropped"
            );
            metrics.record_drop(DropReason::Auth);
            return;
        }
        conn
    };

    // ── FEC decode, per connection ───────────────────────────────
    let decoded = {
        let mut decoder = conn.fec_decoder.lock().await;
        match fec_hdr {
            Some(fh) if fh.is_parity() => {
                metrics.record_fec_parity();
                decoder
                    .receive_parity(fh, bytes::Bytes::copy_from_slice(game_payload))
                    .map(|(_, recovered)| recovered)
            }
            Some(fh) => {
                metrics.record_fec_data();
                let data = bytes::Bytes::copy_from_slice(game_payload);
                Some(decoder.receive_data(fh, data))
            }
            None => Some(bytes::Bytes::copy_from_slice(game_payload)),
        }
    };

    let Some(data) = decoded else {
        // Parity that recovered nothing: nothing to forward.
        return;
    };

    // ── Parse the TCP extension header and route by flags ────────
    let Some((tcp_ext, stream_payload)) = decode_tcp_payload(&data) else {
        debug!(client = %client_addr, "Invalid TCP extension header");
        metrics.record_drop(DropReason::FecMalformed);
        return;
    };

    conn.last_client_seq
        .store(client_header.sequence, Ordering::Relaxed);

    if tcp_ext.flags & tcp_flags::RST != 0 {
        info!(client = %client_addr, "TCP RST, tearing down connection");
        engine.remove_tcp_connection(&tuple).await;
        return;
    }

    // ── Data: non-blocking handoff with a pre-reliability drop valve ──
    //
    // The gate runs BEFORE `on_segment`, so a refused segment never advances
    // the reliability receiver's cumulative ACK: the client sees it as path
    // loss and retransmits on its RTO. Gating after `on_segment` would ACK
    // bytes this relay never enqueued, which is silent byte-stream corruption.
    //
    // A full queue must not block the inbound path either: this function runs
    // in the shared receive loop, so waiting for a slot on one stalled game
    // server would stall every client's traffic.
    if !stream_payload.is_empty() {
        if !conn.has_command_capacity() || !conn.try_reserve_queued_bytes(stream_payload.len()) {
            metrics.record_drop(DropReason::TcpQueueFull);
            return;
        }

        // Relay-measured server leg: a TCP-tunnelled connection has no
        // `ClientSession`, so its forward count lives on the connection. This is
        // the registration point (the same one the UDP path uses below
        // `send_to`); a report reaches the client on the control connection keyed
        // by the same session token. TCP is not excluded from probing. Counted
        // here, after the gate, so a refused segment is not a forwarded one.
        engine.note_server_leg_forward_tcp(&conn, client_header.session_token);
        conn.touch();

        let ordered = conn
            .reliable
            .lock()
            .await
            .receiver
            .on_segment(tcp_ext.tcp_seq, stream_payload);

        if ordered.is_empty() {
            // Out of order: buffered in the reorder window, nothing queued.
            conn.release_queued_bytes(stream_payload.len());
        } else {
            let batch_bytes: usize = ordered.iter().map(bytes::Bytes::len).sum();
            // Filling a reorder gap can release more bytes than this packet
            // carried. The budget is a backpressure bound, not a hard memory
            // fence, so that headroom is admitted; the excess is bounded by the
            // receiver's reorder window.
            let released_extra = batch_bytes.saturating_sub(stream_payload.len());
            if released_extra > 0 {
                conn.queued_bytes
                    .fetch_add(released_extra, Ordering::Relaxed);
            }

            // One `try_send` per inbound packet. This function is the single
            // producer of `Data` batches for this connection and the capacity
            // check above guarantees a free slot; `Full` is unreachable unless
            // that invariant breaks.
            match conn.conn_tx.try_send(TcpCmd::Data(ordered)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // A post-ACK enqueue failure cannot be dropped silently:
                    // reset the client so it never believes the refused bytes
                    // were delivered.
                    conn.release_queued_bytes(batch_bytes);
                    metrics.record_drop(DropReason::TcpQueueFull);
                    reset_tcp_connection(engine, sender, client_header, &tuple, client_addr, fec_k)
                        .await;
                    return;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // The connection task is already gone; drop the stale entry.
                    conn.release_queued_bytes(batch_bytes);
                    metrics.record_drop(DropReason::TcpQueueFull);
                    engine.remove_tcp_connection(&tuple).await;
                    return;
                }
            }
        }
    }

    if tcp_ext.flags & tcp_flags::FIN != 0 {
        conn.touch();
        // A FIN cannot be ordered behind unsent chunks: if the queue has no room
        // for it, reset only this connection instead of stalling the inbound
        // path or closing the write half ahead of unfetched data.
        if conn.conn_tx.try_send(TcpCmd::Fin).is_err() {
            reset_tcp_connection(engine, sender, client_header, &tuple, client_addr, fec_k).await;
        }
    }
}

/// Tear down exactly one TCP connection: send its client a RST and drop the
/// connection. Used when the per-connection handoff cannot accept another
/// command; every other connection is unaffected.
async fn reset_tcp_connection(
    engine: &Arc<RelayEngine>,
    sender: &ClientSender,
    client_header: &TunnelHeader,
    tuple: &Tcp5Tuple,
    client_addr: SocketAddrV4,
    fec_k: u8,
) {
    warn!(
        client = %client_addr,
        src = %tuple.src_ip,
        src_port = tuple.src_port,
        dst = %tuple.dst_ip,
        dst_port = tuple.dst_port,
        "TCP command queue full: resetting only this connection"
    );
    let _ = sender
        .send(&build_tcp_rst_packet(client_header, fec_k))
        .await;
    engine.remove_tcp_connection(tuple).await;
}

/// The per-connection task: dials the real server, splices buffered client
/// payloads into it, reads server bytes back through the reliability encoder
/// and FEC, and tears down. This is the proxy-side connector.
#[allow(clippy::too_many_arguments)]
async fn run_tcp_connection(
    conn: Arc<TcpConnState>,
    client_addr: SocketAddrV4,
    sender: ClientSender,
    client_header: TunnelHeader,
    engine: Arc<RelayEngine>,
    metrics: Arc<ProxyMetrics>,
) {
    let tuple = conn.tuple;
    let dst = SocketAddrV4::new(tuple.dst_ip, tuple.dst_port);
    let upstream = match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(dst)).await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            warn!(client = %client_addr, dst = %dst, error = %e, "TCP dial failed");
            let _ = sender
                .send(&build_tcp_rst_packet(&client_header, conn.fec_k))
                .await;
            engine.remove_tcp_connection(&tuple).await;
            return;
        }
        Err(_) => {
            warn!(client = %client_addr, dst = %dst, "TCP dial timed out");
            let _ = sender
                .send(&build_tcp_rst_packet(&client_header, conn.fec_k))
                .await;
            engine.remove_tcp_connection(&tuple).await;
            return;
        }
    };
    if let Err(e) = upstream.set_nodelay(true) {
        debug!(client = %client_addr, error = %e, "Failed to set TCP_NODELAY");
    }
    let (mut read_half, mut write_half): (OwnedReadHalf, OwnedWriteHalf) = upstream.into_split();
    conn.established.store(true, Ordering::Release);

    let mut rx = conn
        .take_conn_rx()
        .expect("connection task owns the command receiver");
    let mut fec_encoder = FecEncoder::new(conn.fec_k);
    let mut retransmit = tokio::time::interval(Duration::from_millis(50));
    let mut buf = vec![0u8; 2048];
    let mut parity_buf = [0u8; FEC_MAX_PAYLOAD + 2];
    let mut response_seq = 0u16;

    loop {
        tokio::select! {
            _ = conn.cancel.cancelled() => break,
            cmd = rx.recv() => {
                match cmd {
                    Some(TcpCmd::Data(chunks)) => {
                        let mut abort = false;
                        for payload in chunks {
                            match write_upstream_chunk(&conn, &mut write_half, &payload).await {
                                TcpWriteEnd::Written => {}
                                TcpWriteEnd::Cancelled | TcpWriteEnd::Closed => {
                                    abort = true;
                                    break;
                                }
                                TcpWriteEnd::Stalled => {
                                    warn!(
                                        client = %client_addr,
                                        dst = %dst,
                                        "TCP upstream write stalled; resetting only this connection"
                                    );
                                    let _ = sender
                                        .send(&build_tcp_rst_packet(&client_header, conn.fec_k))
                                        .await;
                                    abort = true;
                                    break;
                                }
                            }
                        }
                        if abort {
                            break;
                        }
                    }
                    Some(TcpCmd::Fin) => {
                        let _ = write_half.shutdown().await;
                        // Keep reading until the server closes, then exit.
                        continue;
                    }
                    None => {
                        // The state keeps its sender for the connection's
                        // life, so this only fires if the state itself was
                        // dropped; cancel, upstream EOF and write errors are
                        // the normal teardown paths.
                        break;
                    }
                }
            }
            res = read_half.read(&mut buf) => {
                match res {
                    Ok(n) if n > 0 => {
                        conn.touch();
                        let payload = &buf[..n];
                        let (tcp_seq, tcp_ack) = {
                            let mut reliable = conn.reliable.lock().await;
                            let Some(tcp_seq) = reliable.sender.send(payload) else {
                                continue;
                            };
                            (tcp_seq, reliable.receiver.ack())
                        };
                        let seq = conn.last_client_seq.load(Ordering::Relaxed);
                        let block_id = fec_encoder.block_id();
                        let index = fec_encoder.current_index();
                        let fec_hdr = FecHeader::data(block_id, index, conn.fec_k);
                        let packet = build_tcp_response_packet(
                            &client_header,
                            seq,
                            &fec_hdr,
                            tcp_seq,
                            tcp_ack,
                            tcp_flags::ACK | tcp_flags::PSH,
                            payload,
                        );
                        let _ = sender.send(&packet).await;
                        metrics.record_relay(packet.len() as u64);

                        // FEC: parity emission rides on the same block as the
                        // data packets, matching the client leg's K.
                        let block_complete = fec_encoder.add_packet_inplace(payload);
                        if block_complete {
                            let parity_len = fec_encoder.emit_parity_to(&mut parity_buf);
                            let parity_header = client_header
                                .make_response(response_seq, now_us())
                                .with_session_token(client_header.session_token);
                            let parity_fec = FecHeader::parity(block_id, conn.fec_k);
                            let parity_packet =
                                build_fec_parity_packet(&parity_header, &parity_fec, &parity_buf[..parity_len]);
                            let _ = sender.send(&parity_packet).await;
                            metrics.record_relay(parity_packet.len() as u64);
                            fec_encoder.next_block();
                        }
                        response_seq = response_seq.wrapping_add(1);
                    }
                    Ok(_) => break, // EOF: server closed
                    Err(_) => break,
                }
            }
            _ = retransmit.tick() => {
                let (seq, tcp_ack, payload) = {
                    let mut reliable = conn.reliable.lock().await;
                    let Some((seq, payload)) = reliable.sender.poll_retransmit(Instant::now()) else {
                        continue;
                    };
                    (seq, reliable.receiver.ack(), payload)
                };
                let client_seq = conn.last_client_seq.load(Ordering::Relaxed);
                let fec_hdr = FecHeader::data(
                    fec_encoder.block_id(),
                    fec_encoder.current_index(),
                    conn.fec_k,
                );
                let packet = build_tcp_response_packet(
                    &client_header,
                    client_seq,
                    &fec_hdr,
                    seq,
                    tcp_ack,
                    tcp_flags::ACK | tcp_flags::PSH,
                    &payload,
                );
                let _ = sender.send(&packet).await;
                metrics.record_relay(packet.len() as u64);
            }
        }
    }

    conn.cancel.cancel();
    engine.remove_tcp_connection(&tuple).await;
}

/// Outcome of one write of a queued chunk toward the game server.
enum TcpWriteEnd {
    /// The chunk was fully written; its queue budget is released.
    Written,
    /// The connection was cancelled while the write was in flight.
    Cancelled,
    /// The upstream write failed or the socket closed.
    Closed,
    /// The write made no progress for [`TCP_WRITE_STALL_TIMEOUT`].
    Stalled,
}

/// Write one queued chunk to the game server, concurrently polling connection
/// cancellation and the per-connection stall watchdog.
///
/// The queue byte budget is released only after `write_all` succeeds, so the
/// inbound gate keeps refusing new segments until the upstream has actually
/// accepted the bytes. A write that cannot complete within
/// [`TCP_WRITE_STALL_TIMEOUT`] costs only this connection: the caller sends the
/// client a RST and the task exits.
async fn write_upstream_chunk(
    conn: &TcpConnState,
    write_half: &mut OwnedWriteHalf,
    payload: &[u8],
) -> TcpWriteEnd {
    tokio::select! {
        _ = conn.cancel.cancelled() => TcpWriteEnd::Cancelled,
        result = tokio::time::timeout(TCP_WRITE_STALL_TIMEOUT, write_half.write_all(payload)) => {
            match result {
                Ok(Ok(())) => {
                    conn.release_queued_bytes(payload.len());
                    conn.touch();
                    TcpWriteEnd::Written
                }
                Ok(Err(_)) => TcpWriteEnd::Closed,
                Err(_) => TcpWriteEnd::Stalled,
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Public entry point, dispatches to the platform-appropriate loop
// ─────────────────────────────────────────────────────────────────────────────

/// Run the main relay loop on the data plane socket.
///
/// **Linux**: drains up to `BATCH` (32) packets per `recvmmsg` syscall.
/// **Other platforms**: falls back to one `recv_from` per packet.
///
/// This is the hot path, every game packet goes through here.
#[cfg(target_os = "linux")]
pub async fn run_relay_inbound(
    data_socket: Arc<UdpSocket>,
    engine: Arc<RelayEngine>,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: Arc<RwLock<Authenticator>>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
) -> anyhow::Result<()> {
    let mut batch = BatchState::new();
    configure_data_socket(&data_socket, &metrics);
    info!(
        "Relay inbound loop started (Linux recvmmsg, batch={})",
        BATCH
    );

    loop {
        let n = match recv_batch_async(&data_socket, &mut batch).await {
            Ok(n) => n,
            Err(e) => {
                warn!("Data socket recvmmsg error: {}", e);
                continue;
            }
        };

        // Record batch size for observability (avg batch size = received/batches).
        metrics.record_inbound_batch(n);

        for i in 0..n {
            let pkt_len = batch.msgs[i].msg_len as usize;
            let sin = &batch.saddrs[i];

            // Our socket is AF_INET only; this check is defensive.
            if sin.sin_family as libc::c_int != libc::AF_INET {
                trace!("Ignoring non-AF_INET packet in batch slot {}", i);
                continue;
            }

            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let port = u16::from_be(sin.sin_port);
            let client_addr = SocketAddrV4::new(ip, port);

            process_inbound_packet(
                &batch.bufs[i][..pkt_len],
                client_addr,
                &ClientSender::Udp {
                    socket: Arc::clone(&data_socket),
                    addr: client_addr,
                },
                &engine,
                &rate_limiter,
                &authenticator,
                &abuse_detector,
                &metrics,
            )
            .await;
        }
    }
}

/// Non-Linux fallback: one `recv_from` per packet.
#[cfg(not(target_os = "linux"))]
pub async fn run_relay_inbound(
    data_socket: Arc<UdpSocket>,
    engine: Arc<RelayEngine>,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: Arc<RwLock<Authenticator>>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 2048];
    configure_data_socket(&data_socket, &metrics);
    info!("Relay inbound loop started");

    loop {
        let (len, addr) = match data_socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                warn!("Data socket recv error: {}", e);
                continue;
            }
        };

        // We only support IPv4
        let client_addr = match addr {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => {
                trace!("Ignoring IPv6 packet");
                continue;
            }
        };

        // Record 1 recv_from = 1 batch of 1 packet (for metric consistency).
        metrics.record_inbound_batch(1);

        process_inbound_packet(
            &buf[..len],
            client_addr,
            &ClientSender::Udp {
                socket: Arc::clone(&data_socket),
                addr: client_addr,
            },
            &engine,
            &rate_limiter,
            &authenticator,
            &abuse_detector,
            &metrics,
        )
        .await;
    }
}

/// Record the passive relay→server app-RTT sample for one completed
/// forward/first-response exchange.
///
/// The data path already stored the forward time in
/// [`ClientSession::pending_forward_us`]; this adds no packet and no socket
/// option. The sample is attributed to the session's current data-plane token
/// only: before the session has forwarded enough for a server-leg interest that
/// token is zero and there is no `(session, server)` pair to report against, so
/// a passive figure can never name a server this session has not forwarded to.
/// Bounds and the per-server match are enforced again by
/// [`ServerLegRegistry::record_passive`].
fn record_passive_server_leg(
    registry: &ServerLegRegistry,
    session: &ClientSession,
    now_us: u64,
    marker_us: u64,
) {
    let token = session.server_leg_token.load(Ordering::Relaxed);
    if token == 0 {
        return;
    }
    if let Some(rtt_us) = passive_app_rtt(now_us, marker_us) {
        registry.record_passive(token, *session.game_server.ip(), rtt_us);
    }
}

/// Run the response listener for a single client session.
///
/// Listens on the session's outbound socket for responses from the game
/// server, wraps them in a LightSpeed header, and sends them back to
/// the client via the data plane socket.
///
/// If the client uses FEC, responses are also FEC-encoded.
///
/// `server_leg` is the relay's server-leg registry when probing is enabled. On
/// the first response after a forward the listener records the passive
/// relay→server app-RTT sample from the forward timestamp the data path already
/// stored - no packet is added, it is a timestamp difference.
pub async fn run_session_response_listener(
    session: Arc<ClientSession>,
    metrics: Arc<ProxyMetrics>,
    server_leg: Option<Arc<ServerLegRegistry>>,
) {
    let mut buf = vec![0u8; 2048];

    // Single stack-allocated output buffer reused for every outbound packet.
    // Lives in the task state (heap-allocated by Tokio), zero per-packet alloc.
    // Size: TunnelHeader(20) + FecHeader(4) + max recv buf(2048) = MAX_RELAY_PKT.
    let mut pkt_buf = [0u8; MAX_RELAY_PKT];

    // Create FEC encoder if client supports FEC
    let mut fec_encoder = if session.fec_enabled {
        Some(FecEncoder::new(session.fec_k))
    } else {
        None
    };

    loop {
        // Receive response from game server, bailing early if the session is
        // cancelled (TCP connection dropped or FIN received).
        let len = tokio::select! {
            _ = session.cancel.cancelled() => break,
            res = session.outbound_socket.recv_from(&mut buf) => {
                match res {
                    Ok((len, src_addr)) => {
                        // Drop datagrams from any host other than the game
                        // server (off-path injection guard).
                        if src_addr.ip() != std::net::IpAddr::from(*session.game_server.ip()) {
                            continue;
                        }
                        session.touch();

                        // Record proxy-observed upstream lag exactly once per
                        // forward: `swap(0)` claims the pending marker so later
                        // unprompted server ticks cannot produce samples.
                        let now = session.started_at.elapsed().as_micros() as u64;
                        let marker = session.pending_forward_us.swap(0, Ordering::Relaxed);
                        if marker != 0 {
                            match upstream_lag(now, marker) {
                                Some(lag) => metrics.record_latency(lag),
                                None => metrics.record_latency_discarded(),
                            }
                            // The same gap feeds the passive relay→server
                            // app-RTT sample. It is only attributable to a
                            // session token once the session has forwarded
                            // enough to register a server-leg interest; before
                            // that there is no pair to report against.
                            if let Some(registry) = &server_leg {
                                record_passive_server_leg(registry, &session, now, marker);
                            }
                        }
                        len
                    }
                    Err(e) => {
                        debug!(
                            client = %session.client_addr,
                            error = %e,
                            "Outbound socket recv error"
                        );
                        break;
                    }
                }
            }
        };

        let payload = &buf[..len];
        let seq = session.response_seq.fetch_add(1, Ordering::Relaxed);

        if let Some(ref mut encoder) = fec_encoder {
            // ── FEC mode: zero-alloc data + parity path (WF-008 Item L) ──────
            //
            // Three-phase zero-alloc protocol:
            //   1. add_packet_inplace , XOR into parity accumulator (no alloc)
            //   2. emit_parity_to     , write parity directly into pkt_buf (no alloc)
            //   3. next_block         , reset accumulator for the next block
            //
            // This eliminates both the per-data-packet `Bytes::copy_from_slice`
            // that `add_packet` used to perform and the `BytesMut::with_capacity`
            // that `emit_parity` allocated once per completed block.
            let block_id = encoder.block_id();
            let index = encoder.current_index();

            // Build: [TunnelHeader v2][FecHeader data][game_response]
            // The sequence field echoes the client's last packet sequence (not a
            // proxy-local counter) so the client can deduplicate the same game
            // response arriving via multiple relays in multipath mode.
            let response_header = TunnelHeader::new_fec(
                session.last_client_seq.load(Ordering::Relaxed),
                now_us(),
                session.game_server,
                session.client_addr,
            );
            let fec_hdr = FecHeader::data(block_id, index, session.fec_k);

            // Zero-alloc: write FEC data packet directly into the pre-allocated task buffer.
            let parity_offset = HEADER_SIZE + FEC_HEADER_SIZE;
            let data_end = parity_offset + payload.len();
            pkt_buf[..HEADER_SIZE].copy_from_slice(&response_header.encode_to_array());
            pkt_buf[HEADER_SIZE..parity_offset].copy_from_slice(&fec_hdr.encode_to_array());
            pkt_buf[parity_offset..data_end].copy_from_slice(payload);

            // Phase 1: XOR payload into parity accumulator, no allocation.
            let block_complete = encoder.add_packet_inplace(payload);

            // Send data packet to client (zero heap allocation)
            match session.sender.send(&pkt_buf[..data_end]).await {
                Ok(sent) => {
                    metrics.record_relay(sent as u64);
                    trace!(
                        client = %session.client_addr,
                        seq = seq,
                        fec_block = block_id,
                        fec_idx = index,
                        "Sent FEC response to client"
                    );
                }
                Err(e) => {
                    if session.sender.is_tcp() {
                        break;
                    }
                    debug!(
                        client = %session.client_addr,
                        error = %e,
                        "Failed to send FEC response to client"
                    );
                }
            }

            // Phase 2 + 3: if block complete, emit parity directly into pkt_buf
            // and send, still zero heap allocation.
            if block_complete {
                let parity_seq = session.response_seq.fetch_add(1, Ordering::Relaxed);
                let parity_header = TunnelHeader::new_fec(
                    parity_seq,
                    now_us(),
                    session.game_server,
                    session.client_addr,
                );
                let parity_fec = FecHeader::parity(block_id, session.fec_k);

                // Phase 2: write parity content directly into the pre-allocated buffer.
                // pkt_buf[parity_offset..] is free, the data packet was already sent.
                let parity_len = encoder.emit_parity_to(&mut pkt_buf[parity_offset..]);
                let par_end = parity_offset + parity_len;

                // Write parity packet headers (overwrites the earlier data headers,
                // safe because the data packet was sent above).
                pkt_buf[..HEADER_SIZE].copy_from_slice(&parity_header.encode_to_array());
                pkt_buf[HEADER_SIZE..parity_offset].copy_from_slice(&parity_fec.encode_to_array());

                match session.sender.send(&pkt_buf[..par_end]).await {
                    Ok(sent) => {
                        metrics.record_relay(sent as u64);
                        trace!(
                            client = %session.client_addr,
                            seq = parity_seq,
                            fec_block = block_id,
                            "Sent parity to client (zero-alloc)"
                        );
                    }
                    Err(e) => {
                        if session.sender.is_tcp() {
                            break;
                        }
                        debug!(
                            client = %session.client_addr,
                            error = %e,
                            "Failed to send parity to client"
                        );
                    }
                }

                // Phase 3: advance to the next block.
                encoder.next_block();
            }
        } else {
            // ── Non-FEC mode: zero-alloc response ────────────────────
            let response_header = TunnelHeader::new(
                session.last_client_seq.load(Ordering::Relaxed),
                now_us(),
                session.game_server,
                session.client_addr,
            );

            // Write header + payload directly into the pre-allocated task buffer.
            pkt_buf[..HEADER_SIZE].copy_from_slice(&response_header.encode_to_array());
            pkt_buf[HEADER_SIZE..HEADER_SIZE + payload.len()].copy_from_slice(payload);
            let pkt_end = HEADER_SIZE + payload.len();

            match session.sender.send(&pkt_buf[..pkt_end]).await {
                Ok(sent) => {
                    metrics.record_relay(sent as u64);
                    trace!(
                        client = %session.client_addr,
                        seq = seq,
                        payload_len = len,
                        "Sent response to client"
                    );
                }
                Err(e) => {
                    if session.sender.is_tcp() {
                        break;
                    }
                    debug!(
                        client = %session.client_addr,
                        error = %e,
                        "Failed to send response to client"
                    );
                }
            }
        }
    }
}

/// Periodically clean up expired sessions, stale abuse data, and start
/// response listeners for new sessions.
pub async fn run_session_manager(
    engine: Arc<RelayEngine>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: Arc<RwLock<Authenticator>>,
) {
    let mut known_sessions: HashMap<SocketAddrV4, tokio::task::JoinHandle<()>> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        interval.tick().await;

        // Clean up expired sessions
        let removed = engine.cleanup_expired().await;
        if removed > 0 {
            metrics.record_session_expired(removed as u64);
            info!("Cleaned up {} expired sessions", removed);
        }

        // Clean up expired TCP-tunnelled connections (idle sweep).
        let tcp_removed = engine.cleanup_expired_tcp().await;
        if tcp_removed > 0 {
            metrics.record_session_expired(tcp_removed as u64);
            info!("Cleaned up {} expired TCP connections", tcp_removed);
        }

        // Publish the egress budget state and log any threshold crossing even
        // when no session is being created.
        engine.observe_budget(&metrics);

        // Clean up abuse detector + rate limiter state. Both are keyed by
        // client address and would otherwise grow unbounded from spoofed
        // datagrams (65k source ports per IP).
        {
            let mut abuse = abuse_detector.lock().await;
            abuse.cleanup();
        }
        rate_limiter.lock().await.cleanup();

        // Drop auth entries past their TTL/grace deadline (see auth::sweep).
        authenticator.write().await.sweep(Instant::now());

        // Remove join handles for sessions that no longer exist
        let sessions_lock = engine.sessions();
        let active = sessions_lock.read().await;
        known_sessions.retain(|addr, handle| {
            if !active.contains_key(addr) {
                handle.abort();
                false
            } else {
                true
            }
        });

        // Abort immediately-spawned listeners whose sessions are gone. Their
        // handles are parked in the engine registry, not in `known_sessions`.
        engine.reap_listener_handles(&active).await;

        // Start response listeners for sessions no earlier site claimed. The
        // idempotent claim means a session whose immediate listener already won
        // is skipped here, so exactly one listener runs per session.
        for (addr, session) in active.iter() {
            if !known_sessions.contains_key(addr) && session.claim_response_listener() {
                let session = Arc::clone(session);
                let metrics = Arc::clone(&metrics);
                let server_leg = engine.server_leg.clone();

                let handle = tokio::spawn(async move {
                    run_session_response_listener(session, metrics, server_leg).await;
                });

                known_sessions.insert(*addr, handle);
            }
        }

        let session_count = active.len();
        drop(active);

        if session_count > 0 {
            debug!(
                sessions = session_count,
                listeners = known_sessions.len(),
                "Session manager tick"
            );
        }
    }
}

/// Run the TCP inbound listener for the client→proxy leg.
///
/// Accepts TCP connections on the data-plane port, reads length-prefixed
/// frames, and feeds each frame through the exact same [`process_inbound_packet`]
/// pipeline as UDP, rate limiting, auth, abuse detection, destination
/// validation, and FEC all apply identically.  Responses are written back as
/// framed packets via [`ClientSender::Tcp`].
pub async fn run_tcp_inbound(
    tcp_bind: SocketAddr,
    engine: Arc<RelayEngine>,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: Arc<RwLock<Authenticator>>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
    max_connections: usize,
    read_timeout: Duration,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(tcp_bind).await?;
    info!("TCP inbound listener bound to {}", listener.local_addr()?);

    let semaphore = Arc::new(Semaphore::new(max_connections));

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                warn!("TCP accept error: {}", e);
                continue;
            }
        };

        // The data plane is IPv4-only.
        let peer_v4 = match peer {
            SocketAddr::V4(v4) => v4,
            SocketAddr::V6(_) => {
                trace!(peer = %peer, "Dropping IPv6 TCP client");
                continue;
            }
        };

        // Enforce the concurrent-connection cap.
        let permit = match Arc::clone(&semaphore).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                debug!(client = %peer_v4, "TCP connection limit reached, dropping");
                continue;
            }
        };

        let engine = Arc::clone(&engine);
        let rate_limiter = Arc::clone(&rate_limiter);
        let authenticator = Arc::clone(&authenticator);
        let abuse_detector = Arc::clone(&abuse_detector);
        let metrics = Arc::clone(&metrics);

        tokio::spawn(async move {
            // Hold the permit for the lifetime of the connection.
            let _permit = permit;

            if let Err(e) = stream.set_nodelay(true) {
                warn!(client = %peer_v4, error = %e, "Failed to set TCP_NODELAY");
            }

            let (read, write) = stream.into_split();
            let cancel = CancellationToken::new();
            let sender = ClientSender::Tcp {
                write: Arc::new(tokio::sync::Mutex::new(write)),
                cancel: cancel.clone(),
            };

            let mut read = read;
            let mut buf: Vec<u8> = Vec::with_capacity(MAX_FRAME_SIZE);

            loop {
                let frame =
                    match tokio::time::timeout(read_timeout, read_frame(&mut read, &mut buf)).await
                    {
                        Ok(Ok(Some(n))) => n,
                        Ok(Ok(None)) => break,
                        Ok(Err(e)) => {
                            trace!(client = %peer_v4, error = %e, "TCP read error");
                            break;
                        }
                        Err(_) => {
                            debug!(client = %peer_v4, "TCP read timeout");
                            break;
                        }
                    };

                let fin = process_inbound_packet(
                    &buf[..frame],
                    peer_v4,
                    &sender,
                    &engine,
                    &rate_limiter,
                    &authenticator,
                    &abuse_detector,
                    &metrics,
                )
                .await;

                if fin {
                    break;
                }
            }

            // Connection closed, cancel the session (stops its response
            // listener) and remove it from the engine.
            cancel.cancel();
            engine.sessions().write().await.remove(&peer_v4);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::net::Ipv4Addr;

    /// Build a throwaway UDP [`ClientSender`] for engine unit tests.
    async fn test_udp_sender() -> ClientSender {
        ClientSender::Udp {
            socket: Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap()),
            addr: SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345),
        }
    }

    #[cfg(target_os = "linux")]
    fn test_snapshot(client_addr: &str, game_server: &str, outbound_fd: i32) -> SessionSnapshot {
        SessionSnapshot {
            client_addr: client_addr.to_string(),
            game_server: game_server.to_string(),
            outbound_fd,
            fec_enabled: false,
            fec_k: 4,
            age_us: 1,
            idle_us: 1,
            response_seq: 0,
            last_client_seq: 0,
            packets_relayed: 0,
            bytes_relayed: 0,
        }
    }

    #[tokio::test]
    async fn test_relay_engine_session_lifecycle() {
        let engine = RelayEngine::new(10);

        assert_eq!(engine.active_sessions().await, 0);
        assert!(engine.can_accept().await);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (session, is_new) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);
        assert_eq!(session.client_addr, client);
        assert_eq!(session.game_server, server);
        assert!(!session.fec_enabled);
        assert_eq!(engine.active_sessions().await, 1);

        // Getting same client again should return existing session
        let (session2, is_new2) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(!is_new2);
        assert_eq!(engine.active_sessions().await, 1);
        assert_eq!(session.client_addr, session2.client_addr);
    }

    #[tokio::test]
    async fn test_response_listener_claimed_once() {
        let engine = RelayEngine::new(10);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (sess, is_new) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);

        assert!(
            sess.claim_response_listener(),
            "first claim must win the response listener"
        );
        assert!(
            !sess.claim_response_listener(),
            "second claim must be rejected"
        );
        assert_eq!(sess.response_listeners_spawned(), 1);
    }

    #[tokio::test]
    async fn test_relay_engine_fec_session() {
        let engine = RelayEngine::new(10);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (session, is_new) = engine
            .get_or_create_session(client, server, true, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);
        assert!(session.fec_enabled);
        assert_eq!(session.fec_k, 4);
    }

    #[tokio::test]
    async fn test_relay_engine_max_sessions() {
        let engine = RelayEngine::new(1);

        let client1 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1000);
        let client2 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2000);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        engine
            .get_or_create_session(client1, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(engine
            .get_or_create_session(client2, server, false, 4, test_udp_sender().await)
            .await
            .is_err());
    }

    /// The freeze guard refuses new sessions while an existing session keeps
    /// resolving, then admits new sessions again once unpacked.
    #[tokio::test]
    async fn freeze_handoff_refuses_new_sessions_but_serves_existing() {
        let engine = RelayEngine::new(10);
        let client = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 40_010);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_017);
        let other = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 8), 40_011);
        let sender = test_udp_sender().await;

        let (_, is_new) = engine
            .get_or_create_session(client, server, false, 4, sender.clone())
            .await
            .unwrap();
        assert!(is_new);

        engine.freeze_handoff();
        assert!(engine.is_handoff_frozen());

        let (_, is_new) = engine
            .get_or_create_session(client, server, false, 4, sender.clone())
            .await
            .unwrap();
        assert!(!is_new, "existing session must keep resolving while frozen");

        assert!(
            engine
                .get_or_create_session(other, server, false, 4, sender.clone())
                .await
                .is_err(),
            "new session must be refused while frozen"
        );

        engine.unfreeze_handoff();
        assert!(!engine.is_handoff_frozen());
        let (_, is_new) = engine
            .get_or_create_session(other, server, false, 4, sender)
            .await
            .unwrap();
        assert!(is_new, "new sessions resume after unfreeze");
    }

    /// The egress budget hard threshold refuses only new sessions: an existing
    /// session keeps resolving, a new client is rejected, and the refusal
    /// carries the numbers needed to record why.
    #[tokio::test]
    async fn egress_budget_hard_threshold_refuses_only_new_sessions() {
        let metrics = Arc::new(ProxyMetrics::new());
        let guard = Arc::new(crate::budget::BudgetGuard::new(
            crate::config::BudgetConfig {
                enabled: true,
                max_bytes: 1000,
                monthly_bytes: 0,
                soft_threshold_pct: 80,
                hard_threshold_pct: 100,
            },
        ));
        let engine = RelayEngine::new(10)
            .with_metrics(Arc::clone(&metrics))
            .with_budget(guard);

        let existing = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1000);
        let fresh = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2000);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let sender = test_udp_sender().await;

        engine
            .get_or_create_session(existing, server, false, 4, sender.clone())
            .await
            .unwrap();

        metrics.record_relay(1000);

        let (_, is_new) = engine
            .get_or_create_session(existing, server, false, 4, sender.clone())
            .await
            .unwrap();
        assert!(
            !is_new,
            "an existing session must keep resolving at the hard threshold"
        );

        let err = engine
            .get_or_create_session(fresh, server, false, 4, sender)
            .await
            .unwrap_err();
        let exceeded = err
            .downcast_ref::<crate::budget::BudgetExceeded>()
            .expect("refusal must carry the budget reason");
        assert_eq!(exceeded.limit, 1000);
        assert_eq!(exceeded.used, 1000);
        assert_eq!(exceeded.pct, 100);
    }

    /// A default-configured guard is inert: with no budget configured the
    /// engine admits new sessions no matter how much egress has accumulated.
    #[tokio::test]
    async fn default_budget_is_disabled() {
        let metrics = Arc::new(ProxyMetrics::new());
        let guard = Arc::new(crate::budget::BudgetGuard::new(
            crate::config::BudgetConfig::default(),
        ));
        let engine = RelayEngine::new(10)
            .with_metrics(Arc::clone(&metrics))
            .with_budget(guard);

        metrics.record_relay(u64::MAX / 2);

        let client = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 9), 9000);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let (_, is_new) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .expect("a disabled budget must never refuse a session");
        assert!(is_new);
    }

    /// Activity must extend a session's lifetime: a session touched halfway
    /// through the timeout window survives the first sweep, then expires after
    /// a full idle window with no further touches.
    #[tokio::test]
    async fn test_cleanup_expired_uses_refreshed_activity() {
        let engine = RelayEngine::new_with_timeout(10, Duration::from_millis(60));

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (session, is_new) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);

        // Touch halfway through the window: a refreshed session stays alive.
        tokio::time::sleep(Duration::from_millis(40)).await;
        session.touch();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            engine.cleanup_expired().await,
            0,
            "an activity touch must extend the session lifetime"
        );

        // No further traffic: a full idle window now expires it.
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(engine.cleanup_expired().await, 1);
        assert_eq!(engine.active_sessions().await, 0);
    }

    #[tokio::test]
    async fn test_inbound_decode_and_forward() {
        // Test that we can encode a tunnel packet and decode it
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(1, now_us(), src, dst).with_session_token(42);
        let payload = b"game data payload";

        let packet = header.encode_with_payload(payload);

        let (decoded_header, decoded_payload) = TunnelHeader::decode_with_payload(&packet).unwrap();

        assert_eq!(decoded_header.orig_src_addr(), src);
        assert_eq!(decoded_header.orig_dst_addr(), dst);
        assert_eq!(decoded_header.session_token, 42);
        assert!(!decoded_header.has_fec());
        assert_eq!(decoded_payload, payload);
    }

    #[tokio::test]
    async fn test_fec_packet_decode() {
        // Test FEC v2 packet encoding/decoding
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new_fec(1, now_us(), src, dst);

        let fec_hdr = FecHeader::data(0, 2, 4);
        let game_data = b"game data with fec";

        // Build: [TunnelHeader v2][FecHeader][payload]
        let mut pkt = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + game_data.len());
        pkt.extend_from_slice(&header.encode());
        fec_hdr.encode(&mut pkt);
        pkt.extend_from_slice(game_data);

        // Decode
        let (decoded_header, payload) = TunnelHeader::decode_with_payload(&pkt).unwrap();
        assert!(decoded_header.has_fec());

        // Parse FEC header from payload
        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
        let decoded_fec = FecHeader::decode(&mut fec_slice).unwrap();
        assert_eq!(decoded_fec.block_id, 0);
        assert_eq!(decoded_fec.index, 2);
        assert_eq!(decoded_fec.k_size, 4);
        assert!(!decoded_fec.is_parity());

        let decoded_game_data = &payload[FEC_HEADER_SIZE..];
        assert_eq!(decoded_game_data, game_data);
    }

    /// Verify that the Linux recvmmsg helper correctly receives data.
    ///
    /// Sends several packets to a loopback socket and asserts that
    /// `recv_batch_async` collects them all in (at most) a few calls.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn test_linux_batch_recv_collects_packets() {
        use tokio::net::UdpSocket;

        let recv_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let recv_addr = recv_sock.local_addr().unwrap();

        let send_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        const N: usize = 10;
        // Send N packets before we attempt to receive, so they buffer up.
        for i in 0u8..N as u8 {
            send_sock.send_to(&[i; 64], recv_addr).await.unwrap();
        }

        // Let the kernel queue all N packets.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let mut batch = BatchState::new();
        let mut received = 0usize;

        // Drain with a reasonable timeout, should empty in 1–2 calls.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        while received < N {
            if tokio::time::Instant::now() > deadline {
                break;
            }
            match tokio::time::timeout(
                std::time::Duration::from_millis(100),
                recv_batch_async(&recv_sock, &mut batch),
            )
            .await
            {
                Ok(Ok(n)) => {
                    received += n;
                    // Verify lengths look right
                    for m in batch.msgs.iter().take(n) {
                        assert_eq!(m.msg_len as usize, 64);
                    }
                }
                Ok(Err(e)) => panic!("recv_batch_async error: {}", e),
                Err(_timeout) => break,
            }
        }

        assert_eq!(
            received, N,
            "Expected {} packets via recvmmsg, got {}",
            N, received
        );
    }

    /// Snapshot a UDP session, then adopt it into a fresh engine and verify the
    /// client, game server, FEC parameters, counters, and single response
    /// listener are all reconstructed.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn handoff_snapshot_and_install_reconstruct_udp_sessions() {
        let engine = Arc::new(RelayEngine::new(10));
        let client = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 5), 40_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_015);

        let (session, is_new) = engine
            .get_or_create_session(client, server, true, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);
        session.touch();

        let snapshotter = Arc::clone(&engine);
        let snapshots =
            tokio::task::spawn_blocking(move || snapshotter.snapshot_handoff(Instant::now()))
                .await
                .unwrap();

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].client_addr, client.to_string());
        assert_eq!(snapshots[0].game_server, server.to_string());
        assert!(snapshots[0].fec_enabled);
        assert_eq!(snapshots[0].fec_k, 4);
        assert!(snapshots[0].idle_us <= snapshots[0].age_us);

        // Simulate an `execve`: the outgoing process image disappears, so its
        // reactor registration is gone and the fd stays open for the adopter.
        // `into_std` deregisters the socket; forgetting the std handle keeps the
        // fd alive instead of closing it as the old session is torn down.
        drop(session);
        let original = engine.sessions().write().await.remove(&client).unwrap();
        let original = Arc::try_unwrap(original).unwrap();
        let outbound = Arc::try_unwrap(original.outbound_socket).unwrap();
        std::mem::forget(outbound.into_std().unwrap());

        let adopted = RelayEngine::new(10);
        let data_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let metrics = Arc::new(ProxyMetrics::new());

        let installed = adopted
            .install_handoff_sessions(&snapshots, data_socket, metrics)
            .await;
        assert_eq!(installed, 1);

        let sessions = adopted.sessions();
        let guard = sessions.read().await;
        let restored = guard.get(&client).expect("client session installed");
        assert_eq!(restored.game_server, server);
        assert!(restored.fec_enabled);
        assert_eq!(restored.fec_k, 4);
        assert_eq!(restored.response_listeners_spawned(), 1);
        assert_eq!(restored.pending_forward_us.load(Ordering::Relaxed), 0);
    }

    /// TCP sessions cannot survive an `execve`; they must be skipped.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn handoff_snapshot_skips_tcp_sessions() {
        let engine = Arc::new(RelayEngine::new(10));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();
        let connect = tokio::net::TcpStream::connect(listener_addr);
        let (accepted, connected) = tokio::join!(listener.accept(), connect);
        let (server_stream, _) = accepted.unwrap();
        let _client_stream = connected.unwrap();
        let (_read, write) = server_stream.into_split();

        let sender = ClientSender::Tcp {
            write: Arc::new(tokio::sync::Mutex::new(write)),
            cancel: CancellationToken::new(),
        };
        let client = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 6), 40_001);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_016);
        engine
            .get_or_create_session(client, server, false, 4, sender)
            .await
            .unwrap();

        let snapshotter = Arc::clone(&engine);
        let snapshots =
            tokio::task::spawn_blocking(move || snapshotter.snapshot_handoff(Instant::now()))
                .await
                .unwrap();

        assert!(snapshots.is_empty(), "TCP sessions must be skipped");
    }

    /// A snapshot that is skipped before adoption (here: an unparseable client
    /// address) must have its outbound fd closed rather than leaked. A private
    /// pipe is used as the fd so the close is observable through EPIPE.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn install_handoff_sessions_closes_outbound_fd_for_skipped_snapshot() {
        let mut pipe_fds = [0 as libc::c_int; 2];
        // SAFETY: `pipe` writes two valid descriptors into the array.
        assert_eq!(unsafe { libc::pipe(pipe_fds.as_mut_ptr()) }, 0);
        let (read_end, write_end) = (pipe_fds[0], pipe_fds[1]);

        let engine = RelayEngine::new(10);
        let data_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let metrics = Arc::new(ProxyMetrics::new());
        let snaps = vec![test_snapshot(
            "not-an-address",
            "198.51.100.9:27015",
            read_end,
        )];

        let installed = engine
            .install_handoff_sessions(&snaps, data_socket, metrics)
            .await;
        assert_eq!(installed, 0);
        assert_eq!(engine.active_sessions().await, 0);

        // The read end was closed, so the write end has no reader: EPIPE.
        let byte = [0u8; 1];
        // SAFETY: `write_end` is a live pipe descriptor and `byte` has length 1.
        let rc =
            unsafe { libc::write(write_end, byte.as_ptr() as *const libc::c_void, byte.len()) };
        let write_errno = std::io::Error::last_os_error();
        assert_eq!(
            rc, -1,
            "a skipped snapshot's outbound fd must be closed, not leaked"
        );
        assert_eq!(write_errno.raw_os_error(), Some(libc::EPIPE));
        // SAFETY: `write_end` is still owned by this test.
        unsafe { libc::close(write_end) };
    }

    // ── TCP-tunnelled connection lifecycle ─────────────────────

    fn tcp_tuple(src_port: u16) -> Tcp5Tuple {
        Tcp5Tuple {
            src_ip: Ipv4Addr::new(192, 168, 1, 100),
            src_port,
            dst_ip: Ipv4Addr::new(104, 26, 1, 50),
            dst_port: 25565,
        }
    }

    fn tcp_client(port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 2), port)
    }

    /// Build the `[TcpExtHeader][stream payload]` body a v5 packet carries
    /// after its tunnel and FEC headers: exactly the slice
    /// [`process_tcp_packet`] receives from the inbound pipeline.
    fn tcp_body(tcp_seq: u32, flags: u8, payload: &[u8]) -> bytes::Bytes {
        let ext = TcpExtHeader {
            tcp_seq,
            tcp_ack: 2,
            flags,
            window: 0,
            reserved: [0, 0],
        };
        let mut buf = BytesMut::new();
        ext.encode(&mut buf);
        buf.extend_from_slice(payload);
        buf.freeze()
    }

    /// A client-side UDP receiver plus the relay→client sender socket.
    ///
    /// Production sends client traffic from the relay's data socket, never from
    /// the client's own socket. The two must stay separate here: Windows drops
    /// a substantial fraction of UDP datagrams looped from a socket back to its
    /// own bound address, which would make these tests flaky. Returns the
    /// receiving socket, the client address the relay sends to, and the sender.
    async fn test_client_and_sender() -> (Arc<UdpSocket>, SocketAddrV4, ClientSender) {
        let client_sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client_addr = match client_sock.local_addr().unwrap() {
            SocketAddr::V4(v4) => v4,
            SocketAddr::V6(_) => panic!("expected an IPv4 socket"),
        };
        let sender_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender = ClientSender::Udp {
            socket: Arc::new(sender_sock),
            addr: client_addr,
        };
        (client_sock, client_addr, sender)
    }

    /// A v5 TCP-tunnel header and the 5-tuple [`process_tcp_packet`] derives
    /// from it. Distinct `src_port`s model distinct connections.
    fn tcp_tunnel(src_port: u16, dst: SocketAddrV4) -> (TunnelHeader, Tcp5Tuple) {
        let game_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), src_port);
        let mut header = TunnelHeader::new_fec(1, 1_000_000, game_src, dst);
        header.version = lightspeed_protocol::PROTOCOL_VERSION_TCP;
        let tuple = Tcp5Tuple {
            src_ip: *game_src.ip(),
            src_port: game_src.port(),
            dst_ip: *dst.ip(),
            dst_port: dst.port(),
        };
        (header, tuple)
    }

    /// A game-server stand-in that completes the relay's dial and then never
    /// reads, with a tiny receive buffer so the relay's upstream writes stall
    /// after a few KiB instead of a few MiB of kernel buffering.
    ///
    /// Returns the address to dial; the spawned task holds the accepted socket
    /// open for the rest of the test.
    async fn stalled_tcp_server() -> SocketAddr {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        // A best-effort request: even where the platform ignores it, the
        // never-read socket closes its window and the relay writer still
        // blocks; the request just makes the stall quick and deterministic.
        let _ = socket.set_recv_buffer_size(8 * 1024);
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener = socket.listen(1).unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            // Hold the socket open without ever reading from it.
            std::future::pending::<()>().await;
        });
        addr
    }

    /// A loopback echo server standing in for the game server. Returns its
    /// address and the task handle the caller aborts.
    async fn echo_tcp_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            loop {
                match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        if stream.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });
        (addr, handle)
    }

    /// A game-server stand-in that records every byte the connection task
    /// forwards, for ordered-delivery assertions.
    async fn recording_tcp_server() -> (
        SocketAddr,
        Arc<tokio::sync::Mutex<Vec<u8>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received = Arc::new(tokio::sync::Mutex::new(Vec::<u8>::new()));
        let received_for_task = Arc::clone(&received);
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 512];
            loop {
                match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => received_for_task.lock().await.extend_from_slice(&buf[..n]),
                    _ => break,
                }
            }
        });
        (addr, received, handle)
    }

    /// Wait until `received` holds at least `expected` bytes, or fail.
    async fn wait_for_bytes(received: &tokio::sync::Mutex<Vec<u8>>, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while received.lock().await.len() < expected {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the game server must receive every forwarded byte");
    }

    /// Receive one v5 packet on the client's UDP socket and decode its TCP
    /// extension and stream payload.
    async fn recv_tcp_response(socket: &UdpSocket) -> (TunnelHeader, TcpExtHeader, Vec<u8>) {
        let mut buf = [0u8; 1024];
        let (n, _) = tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf))
            .await
            .expect("a packet must reach the client within 1 s")
            .unwrap();
        let (header, payload) = TunnelHeader::decode_with_payload(&buf[..n]).unwrap();
        assert!(header.has_tcp());
        let (ext, game) = decode_tcp_payload(&payload[FEC_HEADER_SIZE..]).expect("tcp ext");
        (header, ext, game.to_vec())
    }

    /// A v5 TCP-tunneled connection with no connection task: SYN through the
    /// real inbound path would spawn one, so create the state directly.
    async fn create_taskless_connection(
        engine: &Arc<RelayEngine>,
        client_addr: SocketAddrV4,
        tuple: Tcp5Tuple,
    ) -> Arc<TcpConnState> {
        let (conn, is_new) = engine
            .get_or_create_tcp_connection(tuple, client_addr, 4)
            .await
            .unwrap();
        assert!(is_new);
        conn
    }

    #[tokio::test]
    async fn tcp_connection_creation_counts_against_the_session_budget() {
        let engine = RelayEngine::new(1);

        let (conn, is_new) = engine
            .get_or_create_tcp_connection(tcp_tuple(12345), tcp_client(40000), 4)
            .await
            .unwrap();
        assert!(is_new);
        assert_eq!(engine.tcp_connection_count().await, 1);

        // Same tuple resolves to the existing connection, not a new one.
        let (same, is_new_again) = engine
            .get_or_create_tcp_connection(tcp_tuple(12345), tcp_client(40000), 4)
            .await
            .unwrap();
        assert!(!is_new_again);
        assert!(Arc::ptr_eq(&conn, &same));

        // A different 5-tuple would exceed max_sessions (1), so it is refused.
        assert!(engine
            .get_or_create_tcp_connection(tcp_tuple(23456), tcp_client(40000), 4)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_tcp_connection_belongs_to_the_client_that_opened_it() {
        let engine = RelayEngine::new(10);
        let tuple = tcp_tuple(12345);
        let owner = tcp_client(40000);
        let other = tcp_client(40001);

        let (_, is_new) = engine
            .get_or_create_tcp_connection(tuple, owner, 4)
            .await
            .unwrap();
        assert!(is_new);

        // The tuple is client-supplied, so a second client presenting it must
        // not be handed the first client's connection.
        let error = match engine.get_or_create_tcp_connection(tuple, other, 4).await {
            Ok(_) => panic!("a different client must not share the connection"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("another client"), "{error}");
        assert_eq!(engine.tcp_connection_count().await, 1);
    }

    #[tokio::test]
    async fn tcp_idle_sweep_removes_stale_connections() {
        let engine = RelayEngine::new_with_timeout(10, Duration::from_millis(20));

        let (_conn, is_new) = engine
            .get_or_create_tcp_connection(tcp_tuple(12345), tcp_client(40000), 4)
            .await
            .unwrap();
        assert!(is_new);
        assert_eq!(engine.tcp_connection_count().await, 1);

        // Not yet idle past the timeout.
        assert_eq!(engine.cleanup_expired_tcp().await, 0);

        // Idle past the 20 ms timeout: swept, task cancellation token fired.
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(engine.cleanup_expired_tcp().await, 1);
        assert_eq!(engine.tcp_connection_count().await, 0);
    }

    /// A prober that never answers; registration tests never run a burst.
    struct SilentProber;

    impl crate::ping::EchoProber for SilentProber {
        fn echo(&self, _target: Ipv4Addr, _seq: u16) -> Option<u32> {
            None
        }
    }

    fn server_leg_registry() -> Arc<ServerLegRegistry> {
        Arc::new(ServerLegRegistry::new(
            crate::ping::ServerLegConfig::default(),
            Arc::new(crate::ping::SystemClock),
            Arc::new(SilentProber),
        ))
    }

    /// Both data paths register probe interest for their game server through the
    /// same registry and the same session-token key: the UDP session path and
    /// the TCP-tunnelled connection path. Nothing is registered before
    /// [`crate::ping::FORWARD_THRESHOLD`], and a refresh at the same token does
    /// not duplicate the interest.
    #[tokio::test]
    async fn udp_and_tcp_forwarding_both_register_server_leg_interest() {
        let registry = server_leg_registry();
        let engine = RelayEngine::new(10).with_server_leg(Arc::clone(&registry));
        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        // UDP: nothing before the threshold, one interest at it.
        let (session, _) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD - 1 {
            engine.note_server_leg_forward(&session, 0xAB);
        }
        assert_eq!(
            registry.interest_count(),
            0,
            "UDP must not register probe interest before the threshold"
        );
        engine.note_server_leg_forward(&session, 0xAB);
        assert_eq!(
            registry.interest_count(),
            1,
            "UDP forwarding must register the game server as a probe target"
        );
        engine.note_server_leg_forward(&session, 0xAB);
        assert_eq!(
            registry.interest_count(),
            1,
            "a refresh at the same token must not duplicate the UDP interest"
        );

        // TCP: the same threshold and token plumbing on the connection state.
        // A distinct token because the registry keys interest by control-plane
        // session token: this test's UDP session already owns the 0xAB key.
        let tuple = tcp_tuple(12345);
        let (conn, _) = engine
            .get_or_create_tcp_connection(tuple, tcp_client(40000), 4)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD - 1 {
            engine.note_server_leg_forward_tcp(&conn, 0xCD);
        }
        assert_eq!(
            registry.interest_count(),
            1,
            "TCP must not register probe interest before the threshold"
        );
        engine.note_server_leg_forward_tcp(&conn, 0xCD);
        assert_eq!(
            registry.interest_count(),
            2,
            "a TCP-tunnelled connection must register its game server too"
        );

        // The registry's public-unicast gate applies to TCP exactly as to UDP.
        let private_tuple = Tcp5Tuple {
            src_ip: Ipv4Addr::new(192, 168, 1, 100),
            src_port: 12345,
            dst_ip: Ipv4Addr::new(192, 168, 1, 10),
            dst_port: 25565,
        };
        let (private_conn, _) = engine
            .get_or_create_tcp_connection(private_tuple, tcp_client(40000), 4)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD {
            engine.note_server_leg_forward_tcp(&private_conn, 0xAB);
        }
        assert_eq!(
            registry.interest_count(),
            2,
            "a TCP connection to a private address must never be probed"
        );
        assert!(registry.skip_count() >= 1);
    }

    /// A traceroute that never runs: the registration tests never trace.
    struct SilentTraceProber;

    impl crate::trace::TraceProber for SilentTraceProber {
        fn trace(
            &self,
            _target: Ipv4Addr,
            _charge: &mut dyn FnMut(u64) -> bool,
        ) -> Option<crate::trace::Trace> {
            None
        }
    }

    fn trace_registry() -> Arc<TraceRegistry> {
        Arc::new(TraceRegistry::new(
            crate::trace::TraceConfig::default(),
            Arc::new(crate::ping::SystemClock),
            Arc::new(SilentTraceProber),
        ))
    }

    /// The TTL traceroute rides the same data-path interest as the server-leg
    /// prober: the same forward threshold, the same session-token key, the same
    /// public-unicast gate, on both the UDP session and TCP connection paths.
    #[tokio::test]
    async fn udp_and_tcp_forwarding_also_register_trace_interest() {
        let server_leg = server_leg_registry();
        let trace = trace_registry();
        let engine = RelayEngine::new(10)
            .with_server_leg(Arc::clone(&server_leg))
            .with_trace(Arc::clone(&trace));
        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (session, _) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD - 1 {
            engine.note_server_leg_forward(&session, 0xAB);
        }
        assert_eq!(
            trace.interest_count(),
            0,
            "no trace target before the forward threshold"
        );
        engine.note_server_leg_forward(&session, 0xAB);
        assert_eq!(
            trace.interest_count(),
            1,
            "crossing the threshold must register the game server for tracing too"
        );

        let tuple = tcp_tuple(12345);
        let (conn, _) = engine
            .get_or_create_tcp_connection(tuple, tcp_client(40000), 4)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD - 1 {
            engine.note_server_leg_forward_tcp(&conn, 0xCD);
        }
        assert_eq!(trace.interest_count(), 1);
        engine.note_server_leg_forward_tcp(&conn, 0xCD);
        assert_eq!(
            trace.interest_count(),
            2,
            "a TCP-tunnelled connection must register its server for tracing too"
        );

        // The public-unicast gate applies to tracing exactly as to probing.
        let private_tuple = Tcp5Tuple {
            src_ip: Ipv4Addr::new(192, 168, 1, 100),
            src_port: 12345,
            dst_ip: Ipv4Addr::new(192, 168, 1, 10),
            dst_port: 25565,
        };
        let (private_conn, _) = engine
            .get_or_create_tcp_connection(private_tuple, tcp_client(40000), 4)
            .await
            .unwrap();
        for _ in 0..crate::ping::FORWARD_THRESHOLD {
            engine.note_server_leg_forward_tcp(&private_conn, 0xAB);
        }
        assert_eq!(
            trace.interest_count(),
            2,
            "a private destination must never be traced"
        );
        assert!(trace.skip_count() >= 1);
    }

    /// The response listener turns the forward timestamp the data path already
    /// stores into a passive relay→server app-RTT sample: one sample per
    /// forward/first-response exchange, attributed to the session's token, and
    /// nothing for a session with no forwarding history.
    #[tokio::test]
    async fn the_first_response_after_a_forward_feeds_the_passive_app_rtt_window() {
        let registry = server_leg_registry();
        let engine = RelayEngine::new(10).with_server_leg(Arc::clone(&registry));
        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(91, 199, 82, 166), 7777);
        let (session, _) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();

        // Before the forward threshold there is no interest and the session
        // token is zero, so a response contributes nothing.
        record_passive_server_leg(&registry, &session, 150_000, 50_000);
        assert_eq!(registry.passive_sample_count(), 0);

        for _ in 0..crate::ping::FORWARD_THRESHOLD {
            engine.note_server_leg_forward(&session, 0xAB);
        }
        // Exchange 1: forwarded at 50 ms on the session clock, first response
        // at 150 ms -> a 100 ms passive sample for (session, server).
        record_passive_server_leg(&registry, &session, 150_000, 50_000);
        // Exchange 2: 25 ms.
        record_passive_server_leg(&registry, &session, 175_000, 150_000);
        assert_eq!(registry.passive_sample_count(), 2);

        let pushes = registry.due_pushes();
        assert_eq!(pushes.len(), 1, "one app report for this session");
        assert_eq!(pushes[0].0, 0xAB);
        match &pushes[0].1 {
            lightspeed_protocol::control::ControlMessage::ServerLegApp {
                server: reported,
                passive_median_us,
                passive_samples,
                ..
            } => {
                assert_eq!(*reported, *server.ip());
                assert_eq!(*passive_samples, 2);
                assert_eq!(*passive_median_us, 62_500, "median of 100 ms and 25 ms");
            }
            other => panic!("expected a passive server-leg message, got {other:?}"),
        }

        // A response with no packet awaiting one (marker 0) is not a sample.
        record_passive_server_leg(&registry, &session, 200_000, 0);
        assert_eq!(registry.passive_sample_count(), 2);
    }

    /// The connector dials the real server, splices client payloads into it,
    /// and relays server bytes back to the client as v5 packets.
    ///
    /// The packets are fed through [`process_tcp_packet`], the real inbound
    /// routing path, so the connection task's command channel is exercised
    /// exactly as production uses it.
    #[tokio::test]
    async fn tcp_connection_dials_buffers_and_splices() {
        // A local echo server stands in for the game server.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            loop {
                match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        if stream.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });

        let (client_sock, client_addr, sender) = test_client_and_sender().await;

        let game_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let game_dst = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), server_addr.port());
        let tuple = Tcp5Tuple {
            src_ip: *game_src.ip(),
            src_port: game_src.port(),
            dst_ip: *game_dst.ip(),
            dst_port: game_dst.port(),
        };
        let mut header = TunnelHeader::new_fec(1, 1_000_000, game_src, game_dst);
        header.version = lightspeed_protocol::PROTOCOL_VERSION_TCP;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());

        // SYN opens the connection and spawns the connection task.
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;
        assert_eq!(engine.tcp_connection_count().await, 1);

        // The first data segment is buffered until the dial completes and then
        // written to the game server.
        let payload = b"hello minecraft";
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 1, 4)),
            &tcp_body(2, tcp_flags::ACK | tcp_flags::PSH, payload),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;

        // The server's echo comes back as a v5 packet on the client socket.
        let mut recv_buf = [0u8; 512];
        let (n, _) =
            tokio::time::timeout(Duration::from_secs(2), client_sock.recv_from(&mut recv_buf))
                .await
                .expect("server echo must reach the client")
                .unwrap();
        let (resp_header, resp_payload) =
            TunnelHeader::decode_with_payload(&recv_buf[..n]).unwrap();
        assert!(resp_header.has_tcp());
        assert!(resp_header.has_fec());
        // Skip the 4-byte FEC header, decode the TCP extension, and check the
        // game payload round-tripped through the echo server.
        let (tcp_ext, game) =
            decode_tcp_payload(&resp_payload[FEC_HEADER_SIZE..]).expect("tcp ext");
        assert_eq!(
            tcp_ext.tcp_ack,
            2 + payload.len() as u32,
            "the ACK acknowledges the forwarded data bytes"
        );
        assert_eq!(game, payload);

        server.abort();
        engine.remove_tcp_connection(&tuple).await;
    }

    /// Regression (found in R2 review): a TCP-tunnelled connection must keep
    /// forwarding after its first data-bearing packet.
    ///
    /// [`process_tcp_packet`] used to move the command-channel sender out of
    /// [`TcpConnState`] with `take_conn_tx` and drop it, which closed the
    /// channel. The connection task's `rx.recv()` then returned `None`, so it
    /// tore the connection down and dropped every later packet: the connection
    /// survived only its first data-bearing packet.
    #[tokio::test]
    async fn tcp_connection_forwards_every_data_packet_and_stays_alive() {
        // A local server stands in for the game server and records every byte
        // the connection task's writer half actually forwards.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let received = Arc::new(tokio::sync::Mutex::new(Vec::<u8>::new()));
        let received_for_task = Arc::clone(&received);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            loop {
                match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        received_for_task.lock().await.extend_from_slice(&buf[..n]);
                    }
                    _ => break,
                }
            }
        });

        let (_client_sock, client_addr, sender) = test_client_and_sender().await;

        let game_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let game_dst = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), server_addr.port());
        let tuple = Tcp5Tuple {
            src_ip: *game_src.ip(),
            src_port: game_src.port(),
            dst_ip: *game_dst.ip(),
            dst_port: game_dst.port(),
        };
        let mut header = TunnelHeader::new_fec(1, 1_000_000, game_src, game_dst);
        header.version = lightspeed_protocol::PROTOCOL_VERSION_TCP;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());

        // SYN opens the connection and spawns the connection task.
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;

        // Three data-bearing packets on the same connection, in sequence.
        let payloads: [&[u8]; 3] = [b"first", b"-second", b"-third"];
        let mut expected = Vec::new();
        let mut tcp_seq = 2u32;
        for (index, payload) in payloads.iter().enumerate() {
            expected.extend_from_slice(payload);
            process_tcp_packet(
                &header,
                Some(&FecHeader::data(0, index as u8 + 1, 4)),
                &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::PSH, payload),
                client_addr,
                &sender,
                &engine,
                &metrics,
            )
            .await;
            tcp_seq += payload.len() as u32;
        }

        // The writer must receive every payload, in order: not just the first.
        tokio::time::timeout(Duration::from_secs(2), async {
            while received.lock().await.len() < expected.len() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the writer must receive every data-bearing packet, not only the first");
        assert_eq!(*received.lock().await, expected);

        // The connection must still be alive: a forwarded packet may not tear
        // it down.
        assert_eq!(
            engine.tcp_connection_count().await,
            1,
            "forwarding data must not tear the TCP connection down"
        );

        engine.remove_tcp_connection(&tuple).await;
        server.abort();
    }

    /// A FIN shuts the client→server write half but must not tear the whole
    /// connection down: [`run_tcp_connection`] keeps reading server bytes until
    /// the server closes.
    ///
    /// This is the shutdown half of the same lifetime bug: taking and dropping
    /// the sender on FIN closed the command channel, so `rx.recv()` returned
    /// `None` and the connection died before the server could finish.
    #[tokio::test]
    async fn tcp_fin_keeps_the_connection_until_the_server_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let (server_eof_tx, server_eof_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            // Read until the relay shuts its write half (the forwarded FIN).
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            let _ = server_eof_tx.send(());
            // Keep the connection open until the test releases it, so only the
            // relay's own lifetime decision can end the connection.
            let _ = release_rx.await;
            drop(stream);
        });

        let (_client_sock, client_addr, sender) = test_client_and_sender().await;

        let game_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let game_dst = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), server_addr.port());
        let mut header = TunnelHeader::new_fec(1, 1_000_000, game_src, game_dst);
        header.version = lightspeed_protocol::PROTOCOL_VERSION_TCP;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());

        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 1, 4)),
            &tcp_body(2, tcp_flags::ACK | tcp_flags::FIN, b""),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;

        // The FIN reached the writer: the game server sees EOF on its read half.
        tokio::time::timeout(Duration::from_secs(2), server_eof_rx)
            .await
            .expect("the FIN must be forwarded to the game server")
            .expect("the game server task must observe EOF");

        // With the server still holding the connection open, the relay task
        // must stay alive after the FIN.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            engine.tcp_connection_count().await,
            1,
            "a FIN must not tear the connection down before the server closes"
        );

        // Once the server closes, the task ends and removes the connection.
        let _ = release_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), async {
            while engine.tcp_connection_count().await != 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the connection must be torn down after the server closes");
        server.abort();
    }

    /// A game server that stops reading must never stall the shared inbound
    /// path: connection A's queue fills and refuses segments pre-ACK while a
    /// healthy connection B still forwards and gets its echo.
    ///
    /// The failing-before behavior is the blocking per-chunk `send().await`:
    /// once A's 64-slot channel is full, `process_tcp_packet` waits for a slot
    /// and the whole receive loop (every client) stops with it.
    #[tokio::test]
    async fn tcp_stalled_upstream_does_not_block_other_sessions() {
        let stalled_addr = stalled_tcp_server().await;
        let (echo_addr, echo_task) = echo_tcp_server().await;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());

        // Connection A dials a game server that accepts and never reads.
        let (_client_a, client_addr_a, sender_a) = test_client_and_sender().await;
        let (header_a, _tuple_a) = tcp_tunnel(
            12345,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, stalled_addr.port()),
        );
        process_tcp_packet(
            &header_a,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr_a,
            &sender_a,
            &engine,
            &metrics,
        )
        .await;
        assert_eq!(engine.tcp_connection_count().await, 1);

        // Feed far more than one queue's worth of ordered segments. Each call
        // must return promptly: a stalled game server may cost segments (the
        // client retransmits), never the receive loop.
        let payload = [0x5Au8; 1024];
        let mut tcp_seq = 2u32;
        for index in 0..4096u32 {
            let started = Instant::now();
            let call = tokio::time::timeout(
                Duration::from_millis(50),
                process_tcp_packet(
                    &header_a,
                    Some(&FecHeader::data(0, (index % 4) as u8, 4)),
                    &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
                    client_addr_a,
                    &sender_a,
                    &engine,
                    &metrics,
                ),
            )
            .await;
            let elapsed = started.elapsed();
            assert!(
                call.is_ok(),
                "process_tcp_packet blocked for {elapsed:?} (>= 50 ms) at segment {index}: \
                 a stalled upstream must never stall the inbound path"
            );
            assert!(
                elapsed < Duration::from_millis(50),
                "process_tcp_packet took {elapsed:?} (>= 50 ms) at segment {index}"
            );
            tcp_seq = tcp_seq.wrapping_add(payload.len() as u32);
        }

        let drops = metrics.drops_tcp_queue_full.load(Ordering::Relaxed);
        assert!(
            drops > 0,
            "a stalled upstream must exhaust the per-connection queue and record TcpQueueFull, got 0 drops"
        );

        // Connection B is independent: its writer is not blocked, its queue has
        // room, and its echo must arrive while A is still stalled.
        let (client_b, client_addr_b, sender_b) = test_client_and_sender().await;
        let (header_b, _tuple_b) = tcp_tunnel(
            12346,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, echo_addr.port()),
        );
        process_tcp_packet(
            &header_b,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr_b,
            &sender_b,
            &engine,
            &metrics,
        )
        .await;
        let healthy = b"healthy-connection-still-forwards";
        process_tcp_packet(
            &header_b,
            Some(&FecHeader::data(0, 1, 4)),
            &tcp_body(2, tcp_flags::ACK | tcp_flags::PSH, healthy),
            client_addr_b,
            &sender_b,
            &engine,
            &metrics,
        )
        .await;

        let (_resp_header, _ext, echoed) = recv_tcp_response(&client_b).await;
        assert_eq!(
            echoed.as_slice(),
            healthy,
            "a healthy connection must forward and echo within 1 s while another connection's upstream is stalled"
        );

        echo_task.abort();
    }

    /// A segment refused by the pre-ACK gate must not advance the reliability
    /// receiver's cumulative ACK; after the writer drains, re-injecting that
    /// sequence delivers its bytes exactly once and contiguously.
    ///
    /// The failing-before behavior is gating after `on_segment`: the receiver
    /// ACKs the refused bytes and the client never resends them, so the stream
    /// silently loses them.
    #[tokio::test]
    async fn ingress_drop_is_retransmit_recoverable() {
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());
        let (_client_sock, client_addr, sender) = test_client_and_sender().await;
        let (header, tuple) = tcp_tunnel(
            12345,
            SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 25565),
        );
        // No connection task: nothing drains the queue, so it fills
        // deterministically after exactly one channel's worth of batches.
        let conn = create_taskless_connection(&engine, client_addr, tuple).await;

        let payload = [0xA7u8; 64];
        let step = payload.len() as u32;
        let mut tcp_seq = 2u32;
        for index in 0..TCP_CMD_QUEUE_SLOTS {
            process_tcp_packet(
                &header,
                Some(&FecHeader::data(0, (index % 4) as u8, 4)),
                &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
                client_addr,
                &sender,
                &engine,
                &metrics,
            )
            .await;
            tcp_seq = tcp_seq.wrapping_add(step);
        }
        assert!(
            !conn.has_command_capacity(),
            "the queue must be full after one channel of batches"
        );
        assert_eq!(
            metrics.drops_tcp_queue_full.load(Ordering::Relaxed),
            0,
            "nothing may be refused while the queue still has a free slot"
        );

        let ack_before = conn.reliable.lock().await.receiver.ack();
        assert_eq!(
            ack_before, tcp_seq,
            "the accepted prefix must be ACKed contiguously"
        );

        // The next in-order segment is refused by the gate.
        let dropped_seq = tcp_seq;
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(dropped_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;
        assert_eq!(
            metrics.drops_tcp_queue_full.load(Ordering::Relaxed),
            1,
            "the refused segment must be recorded as TcpQueueFull"
        );
        let ack_after = conn.reliable.lock().await.receiver.ack();
        assert_eq!(
            ack_after, ack_before,
            "a pre-ACK queue drop must not advance the cumulative ACK: \
             the client would otherwise never resend the lost bytes"
        );

        // Drain the accepted prefix, as the connection task's writer would.
        let mut rx = conn.take_conn_rx().expect("connection receiver");
        let mut delivered = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                TcpCmd::Data(chunks) => {
                    for chunk in chunks {
                        delivered.extend_from_slice(&chunk);
                    }
                }
                TcpCmd::Fin => panic!("no FIN was queued"),
            }
        }
        assert_eq!(delivered.len(), TCP_CMD_QUEUE_SLOTS * payload.len());
        conn.release_queued_bytes(delivered.len());
        assert_eq!(
            conn.queued_bytes.load(Ordering::Relaxed),
            0,
            "draining the queue must release its byte budget"
        );
        assert!(
            conn.has_command_capacity(),
            "draining the queue must free a slot for the retransmission"
        );

        // The client's RTO retransmits the refused sequence: accepted now,
        // bytes delivered exactly once and contiguous with the prefix.
        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(dropped_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;
        let ack_recovered = conn.reliable.lock().await.receiver.ack();
        assert_eq!(
            ack_recovered,
            ack_before.wrapping_add(step),
            "the retransmitted segment must advance the ACK exactly once"
        );

        let mut recovered = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                TcpCmd::Data(chunks) => {
                    for chunk in chunks {
                        recovered.extend_from_slice(&chunk);
                    }
                }
                TcpCmd::Fin => panic!("no FIN was queued"),
            }
        }
        assert_eq!(
            recovered,
            payload.to_vec(),
            "the retransmitted segment must deliver its bytes exactly once"
        );

        let mut stream = delivered;
        stream.extend_from_slice(&recovered);
        let mut expected = Vec::new();
        for _ in 0..TCP_CMD_QUEUE_SLOTS + 1 {
            expected.extend_from_slice(&payload);
        }
        assert_eq!(
            stream, expected,
            "the recovered stream must be byte-identical and contiguous"
        );
        assert_eq!(
            metrics.packets_dropped.load(Ordering::Relaxed),
            1,
            "only the one refused segment may be counted dropped"
        );
    }

    /// A FIN that cannot be queued behind 64 unsent chunks is not stale data
    /// ordering: only that connection is reset, and a healthy one is untouched.
    #[tokio::test]
    async fn fin_under_congestion_tears_down_only_that_connection() {
        let (echo_addr, echo_task) = echo_tcp_server().await;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());

        // Connection A: its queue is full and nothing is draining it.
        let (client_a, client_addr_a, sender_a) = test_client_and_sender().await;
        let (header_a, tuple_a) = tcp_tunnel(
            12345,
            SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 25565),
        );
        let conn_a = create_taskless_connection(&engine, client_addr_a, tuple_a).await;
        let payload = [0xB4u8; 64];
        let step = payload.len() as u32;
        let mut tcp_seq = 2u32;
        for index in 0..TCP_CMD_QUEUE_SLOTS {
            process_tcp_packet(
                &header_a,
                Some(&FecHeader::data(0, (index % 4) as u8, 4)),
                &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
                client_addr_a,
                &sender_a,
                &engine,
                &metrics,
            )
            .await;
            tcp_seq = tcp_seq.wrapping_add(step);
        }
        assert!(
            !conn_a.has_command_capacity(),
            "the congested connection's queue must be full before the FIN"
        );
        assert_eq!(
            metrics.drops_tcp_queue_full.load(Ordering::Relaxed),
            0,
            "data still fits the queue; only the FIN must not"
        );

        // FIN with a full queue: reset A only.
        process_tcp_packet(
            &header_a,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::FIN, b""),
            client_addr_a,
            &sender_a,
            &engine,
            &metrics,
        )
        .await;

        assert!(
            engine.tcp_table().read().await.get(&tuple_a).is_none(),
            "a FIN that cannot be queued must remove its own connection"
        );
        let (_rst_header, rst_ext, _) = recv_tcp_response(&client_a).await;
        assert!(
            rst_ext.flags & tcp_flags::RST != 0,
            "the client whose connection was reset must see a RST"
        );

        // Connection B is healthy before and after A's reset.
        let (client_b, client_addr_b, sender_b) = test_client_and_sender().await;
        let (header_b, _tuple_b) = tcp_tunnel(
            12346,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, echo_addr.port()),
        );
        process_tcp_packet(
            &header_b,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr_b,
            &sender_b,
            &engine,
            &metrics,
        )
        .await;
        let healthy = b"still-alive";
        process_tcp_packet(
            &header_b,
            Some(&FecHeader::data(0, 1, 4)),
            &tcp_body(2, tcp_flags::ACK | tcp_flags::PSH, healthy),
            client_addr_b,
            &sender_b,
            &engine,
            &metrics,
        )
        .await;
        let (_resp_header, _ext, echoed) = recv_tcp_response(&client_b).await;
        assert_eq!(
            echoed.as_slice(),
            healthy,
            "the FIN reset must not disturb another connection"
        );
        assert_eq!(
            engine.tcp_connection_count().await,
            1,
            "only the congested connection may be torn down"
        );

        echo_task.abort();
    }

    /// Healthy invariance: 200 clean, in-order packets produce zero drops and
    /// arrive in order. The gate must never fire when the writer keeps up.
    #[tokio::test]
    async fn clean_tcp_stream_produces_no_drops_and_ordered_delivery() {
        let (server_addr, received, server_task) = recording_tcp_server().await;
        let engine = Arc::new(RelayEngine::new(10));
        let metrics = Arc::new(ProxyMetrics::new());
        let (_client_sock, client_addr, sender) = test_client_and_sender().await;
        let (header, _tuple) = tcp_tunnel(
            12345,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, server_addr.port()),
        );

        process_tcp_packet(
            &header,
            Some(&FecHeader::data(0, 0, 4)),
            &tcp_body(1, tcp_flags::SYN, b""),
            client_addr,
            &sender,
            &engine,
            &metrics,
        )
        .await;

        let mut expected = Vec::new();
        let mut tcp_seq = 2u32;
        for index in 0..200u32 {
            let payload = [index as u8; 64];
            expected.extend_from_slice(&payload);
            process_tcp_packet(
                &header,
                Some(&FecHeader::data(0, (index % 4) as u8, 4)),
                &tcp_body(tcp_seq, tcp_flags::ACK | tcp_flags::PSH, &payload),
                client_addr,
                &sender,
                &engine,
                &metrics,
            )
            .await;
            // Give the connection task a chance to drain, as the real receive
            // loop does between packets.
            tokio::task::yield_now().await;
            tcp_seq = tcp_seq.wrapping_add(payload.len() as u32);
        }

        wait_for_bytes(&received, expected.len()).await;
        assert_eq!(
            *received.lock().await,
            expected,
            "clean in-order bytes must reach the game server in order"
        );
        assert_eq!(
            metrics.drops_tcp_queue_full.load(Ordering::Relaxed),
            0,
            "200 clean packets must produce zero TcpQueueFull drops"
        );
        assert_eq!(
            metrics.packets_dropped.load(Ordering::Relaxed),
            0,
            "clean TCP traffic must record no drops at all"
        );
        assert_eq!(
            engine.tcp_connection_count().await,
            1,
            "clean traffic must not tear the connection down"
        );

        server_task.abort();
    }

    // ── Proxy-side geo session recording ────────────────────────

    fn test_geo_resolver() -> Arc<dyn GeoResolver> {
        Arc::new(|ip: Ipv4Addr| {
            if ip == Ipv4Addr::new(198, 51, 100, 9) {
                Some("DE".to_string())
            } else {
                Some("US".to_string())
            }
        })
    }

    fn test_geo_engine(metrics: &Arc<ProxyMetrics>, public_ip: Option<Ipv4Addr>) -> RelayEngine {
        RelayEngine::new(10).with_geo(GeoState {
            resolver: test_geo_resolver(),
            metrics: Arc::clone(metrics),
            public_ip,
            control_port: 4433,
            data_port: 4434,
        })
    }

    #[tokio::test]
    async fn new_session_records_country_pair() {
        let metrics = Arc::new(ProxyMetrics::new());
        let engine = test_geo_engine(&metrics, None);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 10), 50_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_015);

        let (_, is_new) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(is_new);

        let agg = metrics.geo.lock().unwrap();
        assert_eq!(
            agg.cells.get(&("US".to_string(), "DE".to_string())),
            Some(&1),
            "a new session must contribute one (src,dst) country pair"
        );
    }

    #[tokio::test]
    async fn existing_session_not_double_counted() {
        let metrics = Arc::new(ProxyMetrics::new());
        let engine = test_geo_engine(&metrics, None);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 10), 50_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_015);

        let (_, first) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        let (_, second) = engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();
        assert!(first && !second);

        let agg = metrics.geo.lock().unwrap();
        assert_eq!(
            agg.cells.get(&("US".to_string(), "DE".to_string())),
            Some(&1)
        );
    }

    #[tokio::test]
    async fn self_tunnel_skipped_by_public_ip() {
        let metrics = Arc::new(ProxyMetrics::new());
        let engine = test_geo_engine(&metrics, Some(Ipv4Addr::new(198, 51, 100, 9)));

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 10), 50_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 27_015);

        engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();

        let agg = metrics.geo.lock().unwrap();
        assert!(agg.cells.is_empty(), "a self-tunnel must not record a pair");
        assert_eq!(agg.skipped_self_tunnel, 1);
    }

    #[tokio::test]
    async fn self_tunnel_skipped_by_port_heuristic() {
        let metrics = Arc::new(ProxyMetrics::new());
        let engine = test_geo_engine(&metrics, None);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 10), 50_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 4433);

        engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();

        let agg = metrics.geo.lock().unwrap();
        assert!(agg.cells.is_empty(), "a self-tunnel must not record a pair");
        assert_eq!(agg.skipped_self_tunnel, 1);
    }

    #[tokio::test]
    async fn data_port_destination_is_not_skipped() {
        // Regression: the fallback heuristic must not treat a legitimate
        // destination on the data port as a self-tunnel, or it would hide
        // real demand on port 4434.
        let metrics = Arc::new(ProxyMetrics::new());
        let engine = test_geo_engine(&metrics, None);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 10), 50_000);
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 4434);

        engine
            .get_or_create_session(client, server, false, 4, test_udp_sender().await)
            .await
            .unwrap();

        let agg = metrics.geo.lock().unwrap();
        assert_eq!(
            agg.cells.get(&("US".to_string(), "DE".to_string())),
            Some(&1),
            "a destination on the data port must still be recorded"
        );
        assert_eq!(agg.skipped_self_tunnel, 0);
    }

    // ── Socket hygiene (WIN 1) ──────────────────────────────────

    #[test]
    fn socket_buffer_requests_are_clamped_to_sane_bounds() {
        assert_eq!(clamp_buf(1), SOCK_BUF_MIN as u32);
        assert_eq!(clamp_buf(usize::MAX), SOCK_BUF_MAX as u32);
        assert_eq!(clamp_buf(128 * 1024), 128 * 1024);
        assert!(SocketRole::Data.rcvbuf() >= SocketRole::Session.rcvbuf());
    }

    /// A refused option must be counted and skipped, never fatal: `fd = -1`
    /// makes all three `setsockopt` calls fail with EBADF.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refused_socket_option_is_recorded_and_non_fatal() {
        let metrics = ProxyMetrics::new();
        let report = configure_socket_fd(-1, SocketRole::Data, &metrics);
        assert_eq!(
            report.failures, 3,
            "receive buffer, send buffer and busy-poll are each attempted once"
        );
        assert!(report.rcvbuf_effective.is_none());
        assert!(report.sndbuf_effective.is_none());
        assert!(report.busy_poll.is_none());
        assert_eq!(metrics.socket_opts_failed.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.socket_opts_applied.load(Ordering::Relaxed), 0);
    }

    /// A bound socket accepts the buffers and the result is observable in the
    /// Prometheus surface. Busy-poll may or may not be supported, so only the
    /// buffer counters are asserted.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn data_socket_tuning_applies_and_is_observable() {
        let metrics = ProxyMetrics::new();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let report = configure_socket(&socket, SocketRole::Data, &metrics);

        assert!(
            report.rcvbuf_effective.unwrap() >= SOCK_BUF_MIN as u32,
            "the kernel must return at least the floor it was asked for"
        );
        assert!(report.sndbuf_effective.is_some());
        assert!(metrics.socket_opts_applied.load(Ordering::Relaxed) >= 2);

        let prom = metrics.to_prometheus("test", "node");
        assert!(prom.contains("lightspeed_socket_opts_applied_total"));
        assert!(prom.contains("lightspeed_socket_opts_failed_total"));
        assert!(prom.contains("lightspeed_busy_poll_sockets"));
    }

    /// The data socket is tuned before the receive loop starts; the existing
    /// `recvmmsg` path must still drain every queued datagram afterwards.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn tuning_does_not_disturb_the_recvmmsg_path() {
        let recv_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let metrics = ProxyMetrics::new();
        configure_data_socket(&recv_sock, &metrics);
        let recv_addr = recv_sock.local_addr().unwrap();

        let send_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        for i in 0u8..8 {
            send_sock.send_to(&[i; 32], recv_addr).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let mut batch = BatchState::new();
        let n = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            recv_batch_async(&recv_sock, &mut batch),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(n >= 1);
        for m in batch.msgs.iter().take(n) {
            assert_eq!(m.msg_len as usize, 32);
        }
    }
}
