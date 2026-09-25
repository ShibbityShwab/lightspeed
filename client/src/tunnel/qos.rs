//! # Tunnel QoS (DSCP) marking
//!
//! Optionally marks tunnel UDP sockets with IPv4 DSCP Expedited Forwarding
//! (EF, 46) so a player's own router or ISP can prioritise the tunnel when it
//! honours the field. This is a best-effort hint on the player's local hop
//! only: every hop past the first router is free to ignore, reclassify, or
//! penalise it, and the proxy side is not marked at all.
//!
//! Marking is **opt-in**. It is off by default because it changes the QoS
//! class of the user's traffic without their network's consent: some ISPs
//! police or drop DSCP 46 from unknown sources, and a third-party app should
//! not stamp EF on traffic by default. Enable with `dscp = true` in
//! `lightspeed.toml` or the `--dscp` flag.
//!
//! Windows is intentionally a no-op: modern Windows ignores `IP_TOS` set
//! through Winsock and requires the much heavier QWAVE/QoS2 API to mark
//! egress traffic, so we do not pretend to support it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::net::UdpSocket;

/// DSCP Expedited Forwarding (EF, decimal 46), the standard low-latency class.
///
/// The DSCP value occupies the top six bits of the IPv4 TOS byte, so the TOS
/// byte written to the socket is `DSCP_EF << 2` (184).
pub const DSCP_EF: u8 = 46;

/// Process-global opt-in switch, set once from resolved config at startup.
static DSCP_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable DSCP marking for sockets created from now on.
///
/// Called once from `main` after CLI and config are resolved. Read at each
/// socket setup; existing sockets are not retroactively changed.
pub fn set_enabled(enabled: bool) {
    DSCP_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether DSCP marking is enabled for newly created tunnel sockets.
pub fn enabled() -> bool {
    DSCP_ENABLED.load(Ordering::Relaxed)
}

/// Apply the configured DSCP marking to `socket`.
///
/// No-op when marking is disabled. A failure is returning an `io::Error` so
/// callers can log it, but it is never fatal: marking is a best-effort hint.
pub fn apply(socket: &UdpSocket) -> io::Result<()> {
    if !enabled() {
        return Ok(());
    }
    set_dscp(socket, DSCP_EF)
}

/// Write `dscp` into the socket's IPv4 TOS field.
pub fn set_dscp(socket: &UdpSocket, dscp: u8) -> io::Result<()> {
    // The DSCP value sits in the top six bits of the TOS byte; the low two
    // bits are ECN, which we leave at zero.
    let tos = i32::from(dscp) << 2;
    #[cfg(target_os = "linux")]
    {
        linux_set_tos(socket, tos)
    }
    #[cfg(target_os = "macos")]
    {
        macos_set_tos(socket, tos)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // Windows ignores IP_TOS set through Winsock without the QWAVE/QoS2
        // API; see the module docs. Other targets are left unmarked.
        let _ = (socket, tos);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn linux_set_tos(socket: &UdpSocket, tos: i32) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let value: libc::c_int = tos;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_TOS's documented `c_int` payload.
    let rc = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_TOS,
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
fn macos_set_tos(socket: &UdpSocket, tos: i32) -> io::Result<()> {
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
    const IP_TOS: c_int = 3;
    let value: c_int = tos;
    // SAFETY: `socket` owns a live fd for the duration of the call, and the
    // value plus its size match IP_TOS's documented `c_int` payload.
    let rc = unsafe {
        setsockopt(
            socket.as_raw_fd(),
            IPPROTO_IP,
            IP_TOS,
            std::ptr::addr_of!(value).cast::<c_void>(),
            std::mem::size_of::<c_int>() as c_uint,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    mod linux {
        use std::os::fd::AsRawFd;

        use super::super::*;

        /// Read the raw IPv4 TOS byte the kernel has on the socket.
        fn socket_tos(socket: &UdpSocket) -> i32 {
            let mut value: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            // SAFETY: `socket` owns a live fd and the value/len pair matches
            // IP_TOS's documented `c_int` payload.
            let rc = unsafe {
                libc::getsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_IP,
                    libc::IP_TOS,
                    std::ptr::addr_of_mut!(value).cast::<libc::c_void>(),
                    &mut len,
                )
            };
            assert_eq!(rc, 0, "getsockopt(IP_TOS) failed");
            value
        }

        #[tokio::test]
        async fn dscp_ef_is_written_only_when_enabled() {
            let std_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind");
            std_sock.set_nonblocking(true).expect("nonblocking");
            let socket = UdpSocket::from_std(std_sock).expect("tokio socket");

            set_enabled(false);
            apply(&socket).expect("apply disabled");
            assert_eq!(
                socket_tos(&socket),
                0,
                "disabled DSCP marking must leave IP_TOS untouched"
            );

            set_enabled(true);
            apply(&socket).expect("apply enabled");
            assert_eq!(
                socket_tos(&socket),
                i32::from(DSCP_EF) << 2,
                "DSCP EF (46) must be written to the top six bits of IP_TOS"
            );

            set_enabled(false);
        }
    }
}
