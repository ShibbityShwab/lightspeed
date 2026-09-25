//! # Tunnel Transport
//!
//! Abstracts the client↔proxy leg of the tunnel over UDP or TCP.  Game
//! traffic stays UDP; the client↔proxy leg may use TCP when the network
//! blocks UDP.  Over TCP, each tunnel packet is wrapped in a length-prefixed
//! frame so the byte stream preserves packet boundaries.

use std::io;
use std::net::SocketAddrV4;
use std::sync::{Arc, RwLock};

use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;

use lightspeed_protocol::framing::{read_frame, write_frame};

/// Request the kernel set the don't-fragment (DF) bit on a tunnel UDP socket.
///
/// With DF set, a datagram larger than the path MTU is rejected with
/// `EMSGSIZE` (surfaced as a send error) instead of being fragmented on the
/// wire. Linux uses `IP_MTU_DISCOVER = IP_PMTUDISC_DO`; macOS uses
/// `IP_DONTFRAG`; Windows sets `IP_DONTFRAGMENT` to TRUE. A failure to set the
/// flag is non-fatal: the payload budget is the primary guarantee and DF is
/// defence in depth.
///
/// This is the default policy for every tunnel socket. The one exception is a
/// payload over the conservative budget, which is sent with DF temporarily
/// cleared (see [`set_fragment_allowed`] and [`FragmentAllowed`]) so the kernel
/// may fragment it rather than lose it.
pub fn set_dont_fragment(socket: &UdpSocket) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_set_dont_fragment(socket)
    }
    #[cfg(target_os = "macos")]
    {
        macos_set_dont_fragment(socket)
    }
    #[cfg(windows)]
    {
        windows_set_dontfragment(socket, true)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = socket;
        Ok(())
    }
}

/// Allow the kernel to fragment datagrams sent on a tunnel UDP socket.
///
/// Linux uses `IP_MTU_DISCOVER = IP_PMTUDISC_DONT`; macOS clears `IP_DONTFRAG`;
/// Windows clears `IP_DONTFRAGMENT`. This is used only around the rare
/// oversized payload so the conservative no-fragment guarantee stays intact
/// for payloads that fit.
pub fn set_fragment_allowed(socket: &UdpSocket) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_set_fragment_allowed(socket)
    }
    #[cfg(target_os = "macos")]
    {
        macos_set_fragment_allowed(socket)
    }
    #[cfg(windows)]
    {
        windows_set_dontfragment(socket, false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = socket;
        Ok(())
    }
}

/// Allow kernel fragmentation on a tunnel UDP socket until the guard drops,
/// then restore don't-fragment.
///
/// Only used for a payload over the conservative budget. Game-payload sends on
/// one data plane are sequential, so clearing the bit cannot race another
/// game-payload send; the keepalive task shares the socket but only emits
/// small control packets that never approach the MTU.
pub struct FragmentAllowed<'a> {
    socket: &'a UdpSocket,
}

impl<'a> FragmentAllowed<'a> {
    /// Clear don't-fragment on `socket` until the guard is dropped.
    pub fn new(socket: &'a UdpSocket) -> io::Result<Self> {
        set_fragment_allowed(socket)?;
        Ok(Self { socket })
    }
}

impl Drop for FragmentAllowed<'_> {
    fn drop(&mut self) {
        if let Err(e) = set_dont_fragment(self.socket) {
            tracing::warn!("Could not restore don't-fragment on tunnel socket: {e}");
        }
    }
}

/// Send one already-encoded tunnel datagram, allowing the local kernel to
/// fragment it when it exceeds the conservative clamp.
///
/// Datagrams that fit keep the socket's don't-fragment guarantee. Used by the
/// interceptor backends and capture mode, which send on a raw socket.
pub async fn send_datagram(
    socket: &UdpSocket,
    packet: &[u8],
    addr: SocketAddrV4,
) -> io::Result<usize> {
    if crate::tunnel::budget::datagram_fits(packet.len()) {
        socket.send_to(packet, addr).await
    } else {
        let _allow = FragmentAllowed::new(socket)?;
        socket.send_to(packet, addr).await
    }
}

#[cfg(target_os = "linux")]
fn linux_set_dont_fragment(socket: &UdpSocket) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let value: libc::c_int = libc::IP_PMTUDISC_DO;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_MTU_DISCOVER's documented `c_int` payload.
    let rc = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            std::ptr::addr_of!(value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_set_fragment_allowed(socket: &UdpSocket) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let value: libc::c_int = libc::IP_PMTUDISC_DONT;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_MTU_DISCOVER's documented `c_int` payload.
    let rc = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            std::ptr::addr_of!(value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_set_dont_fragment(socket: &UdpSocket) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_int, c_uint, c_void};

    extern "C" {
        fn setsockopt(
            fd: c_int,
            level: c_int,
            optname: c_int,
            optval: *const c_void,
            optlen: c_uint,
        ) -> c_int;
    }

    const IPPROTO_IP: c_int = 0;
    const IP_DONTFRAG: c_int = 28;
    let value: c_int = 1;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_DONTFRAG's documented `c_int` payload.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_fd(),
            IPPROTO_IP,
            IP_DONTFRAG,
            std::ptr::addr_of!(value).cast::<c_void>(),
            std::mem::size_of::<c_int>() as c_uint,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_set_fragment_allowed(socket: &UdpSocket) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_int, c_uint, c_void};

    extern "C" {
        fn setsockopt(
            fd: c_int,
            level: c_int,
            optname: c_int,
            optval: *const c_void,
            optlen: c_uint,
        ) -> c_int;
    }

    const IPPROTO_IP: c_int = 0;
    const IP_DONTFRAG: c_int = 28;
    let value: c_int = 0;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_DONTFRAG's documented `c_int` payload.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_fd(),
            IPPROTO_IP,
            IP_DONTFRAG,
            std::ptr::addr_of!(value).cast::<c_void>(),
            std::mem::size_of::<c_int>() as c_uint,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Windows has defaulted UDP sockets to don't-fragment (DF) since Vista, and
/// exposes `IP_DONTFRAGMENT` as a `DWORD`-valued socket option. Clearing it
/// lets the stack fragment an oversized datagram; setting it restores the
/// conservative default.
///
/// NOTE: this branch is not built or exercised on the Linux/macOS build hosts,
/// so it is the one platform edit that remains unverified.
#[cfg(windows)]
fn windows_set_dontfragment(socket: &UdpSocket, enabled: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;

    const IPPROTO_IP: i32 = 0;
    const IP_DONTFRAGMENT: i32 = 14;

    #[link(name = "ws2_32")]
    extern "system" {
        fn setsockopt(s: usize, level: i32, optname: i32, optval: *const u8, optlen: i32) -> i32;
    }

    let value: u32 = u32::from(enabled);
    // SAFETY: `socket` owns a live SOCKET for the duration of the call, and
    // IP_DONTFRAGMENT takes a DWORD payload whose size we pass explicitly.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            IPPROTO_IP,
            IP_DONTFRAGMENT,
            std::ptr::addr_of!(value).cast::<u8>(),
            std::mem::size_of::<u32>() as i32,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Transport for the client↔proxy leg of the tunnel.
pub enum TunnelTransport {
    Udp {
        socket: Arc<UdpSocket>,
        proxy: SocketAddrV4,
    },
    Tcp {
        sender: Arc<Mutex<OwnedWriteHalf>>,
        reader: OwnedReadHalf,
        proxy: SocketAddrV4,
    },
}

impl TunnelTransport {
    /// Bind a UDP transport on `local`.
    pub async fn connect_udp(local: SocketAddrV4) -> io::Result<Self> {
        let socket = UdpSocket::bind(local).await?;
        if let Err(e) = set_dont_fragment(&socket) {
            tracing::warn!("Could not set don't-fragment on tunnel UDP socket: {e}");
        }
        if let Err(e) = crate::tunnel::qos::apply(&socket) {
            tracing::warn!("Could not set DSCP on tunnel UDP socket: {e}");
        }
        Ok(Self::Udp {
            socket: Arc::new(socket),
            proxy: SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0),
        })
    }

    /// Connect a TCP transport to `proxy` and enable `TCP_NODELAY`.
    pub async fn connect_tcp(proxy: SocketAddrV4) -> io::Result<Self> {
        let stream = TcpStream::connect(proxy).await?;
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();
        Ok(Self::Tcp {
            sender: Arc::new(Mutex::new(writer)),
            reader,
            proxy,
        })
    }

    /// Set the peer address (no-op for TCP, where the peer is fixed at connect).
    pub fn set_proxy(&mut self, proxy: SocketAddrV4) {
        match self {
            Self::Udp { proxy: p, .. } | Self::Tcp { proxy: p, .. } => *p = proxy,
        }
    }

    /// The peer address (the proxy).
    pub fn proxy_addr(&self) -> SocketAddrV4 {
        match self {
            Self::Udp { proxy, .. } | Self::Tcp { proxy, .. } => *proxy,
        }
    }

    /// Local bind address of the UDP socket (the TCP variant has none).
    pub fn local_addr(&self) -> io::Result<SocketAddrV4> {
        match self {
            Self::Udp { socket, .. } => match socket.local_addr()? {
                std::net::SocketAddr::V4(v4) => Ok(v4),
                std::net::SocketAddr::V6(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "IPv6 local address",
                )),
            },
            Self::Tcp { .. } => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "TCP transport has no datagram local address",
            )),
        }
    }

    /// Send `bytes` to the proxy.
    pub async fn send(&self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Udp { socket, proxy } => socket.send_to(bytes, *proxy).await,
            Self::Tcp { sender, .. } => {
                let mut guard = sender.lock().await;
                write_frame(&mut *guard, bytes).await?;
                Ok(bytes.len())
            }
        }
    }

    /// Send `bytes` to the proxy, allowing the local kernel to fragment this
    /// datagram.
    ///
    /// Only used for a payload over the conservative budget: the socket's
    /// don't-fragment bit is cleared for the send and restored afterwards, so
    /// payloads that fit keep the no-fragment guarantee.
    pub async fn send_may_fragment(&self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Udp { socket, proxy } => {
                let _allow = FragmentAllowed::new(socket)?;
                socket.send_to(bytes, *proxy).await
            }
            Self::Tcp { .. } => self.send(bytes).await,
        }
    }

    /// Receive one tunnel packet from the proxy into `buf`.
    ///
    /// Returns `Ok(Some(n))` with the packet in `buf[..n]`, or `Ok(None)` on a
    /// clean TCP close.  Requires `&mut self` because the TCP read half is
    /// exclusive.
    pub async fn recv(&mut self, buf: &mut Vec<u8>) -> io::Result<Option<usize>> {
        match self {
            Self::Udp { socket, proxy } => {
                let (n, addr) = socket.recv_from(buf).await?;
                match addr {
                    std::net::SocketAddr::V4(v4) => *proxy = v4,
                    std::net::SocketAddr::V6(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "IPv6 proxy not supported",
                        ));
                    }
                }
                Ok(Some(n))
            }
            Self::Tcp { reader, .. } => read_frame(reader, buf).await,
        }
    }

    /// Split into a shared send half and an exclusive read half.
    pub fn split(self) -> (TunnelSender, TunnelReader) {
        match self {
            Self::Udp { socket, proxy } => (
                TunnelSender::Udp(Arc::clone(&socket), Arc::new(RwLock::new(proxy))),
                TunnelReader::Udp(socket),
            ),
            Self::Tcp { sender, reader, .. } => {
                (TunnelSender::Tcp(sender), TunnelReader::Tcp(reader))
            }
        }
    }
}

/// Shared send half of a tunnel transport, cloneable for outbound tasks.
///
/// For UDP the proxy destination is held behind a lock so it can be switched
/// mid-session (continuous re-routing) without rebuilding the tunnel.
#[derive(Clone)]
pub enum TunnelSender {
    Udp(Arc<UdpSocket>, Arc<RwLock<SocketAddrV4>>),
    Tcp(Arc<Mutex<OwnedWriteHalf>>),
}

impl TunnelSender {
    /// Send `bytes` to the proxy.
    pub async fn send(&self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Udp(socket, proxy) => {
                let proxy = *proxy.read().unwrap();
                socket.send_to(bytes, proxy).await
            }
            Self::Tcp(sender) => {
                let mut guard = sender.lock().await;
                write_frame(&mut *guard, bytes).await?;
                Ok(bytes.len())
            }
        }
    }

    /// Send `bytes` to the proxy, allowing the local kernel to fragment this
    /// datagram. Counterpart to [`TunnelTransport::send_may_fragment`], used
    /// only for a payload over the conservative budget.
    pub async fn send_may_fragment(&self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Udp(socket, proxy) => {
                let _allow = FragmentAllowed::new(socket)?;
                let proxy = *proxy.read().unwrap();
                socket.send_to(bytes, proxy).await
            }
            Self::Tcp(_) => self.send(bytes).await,
        }
    }

    /// Switch the UDP proxy destination in place (no-op for TCP, where the
    /// peer is fixed at connect time).
    pub fn set_proxy(&self, proxy: SocketAddrV4) {
        if let Self::Udp(_, p) = self {
            *p.write().unwrap() = proxy;
        }
    }

    /// The current proxy destination.
    pub fn proxy_addr(&self) -> Option<SocketAddrV4> {
        match self {
            Self::Udp(_, p) => Some(*p.read().unwrap()),
            Self::Tcp(_) => None,
        }
    }
}

/// Exclusive read half of a tunnel transport, owned by the inbound task.
pub enum TunnelReader {
    Udp(Arc<UdpSocket>),
    Tcp(OwnedReadHalf),
}

impl TunnelReader {
    /// Receive one tunnel packet from the proxy into `buf`.
    pub async fn recv(&mut self, buf: &mut Vec<u8>) -> io::Result<Option<usize>> {
        match self {
            Self::Udp(socket) => {
                let (n, _addr) = socket.recv_from(buf).await?;
                Ok(Some(n))
            }
            Self::Tcp(reader) => read_frame(reader, buf).await,
        }
    }

    /// Whether this read half is TCP.
    pub fn is_tcp(&self) -> bool {
        matches!(self, Self::Tcp(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_udp_transport_roundtrip() {
        let echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = match echo.local_addr().unwrap() {
            std::net::SocketAddr::V4(v4) => v4,
            _ => panic!("expected IPv4"),
        };

        let echo_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            let (n, addr) = echo.recv_from(&mut buf).await.unwrap();
            echo.send_to(&buf[..n], addr).await.unwrap();
        });
        let mut transport = TunnelTransport::connect_udp(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        transport.set_proxy(echo_addr);
        transport.send(b"udp ping").await.unwrap();

        let mut buf = vec![0u8; 2048];
        let n = transport.recv(&mut buf).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"udp ping");
        echo_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_transport_roundtrip() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = match listener.local_addr().unwrap() {
            std::net::SocketAddr::V4(v4) => v4,
            _ => panic!("expected IPv4"),
        };

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let n = read_frame(&mut stream, &mut buf).await.unwrap().unwrap();
            (buf, n)
        });

        let transport = TunnelTransport::connect_tcp(proxy).await.unwrap();
        let payload = b"tcp roundtrip";
        let sent = transport.send(payload).await.unwrap();
        assert_eq!(sent, payload.len());

        let (buf, n) = server.await.unwrap();
        assert_eq!(n, payload.len());
        assert_eq!(&buf, payload);
    }
}
