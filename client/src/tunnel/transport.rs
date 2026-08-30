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
