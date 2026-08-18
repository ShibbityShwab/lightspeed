//! # TCP Framing
//!
//! Length-prefixed framing for the client→proxy TCP leg of the data plane.
//! Each frame is a 4-byte big-endian length prefix followed by a single tunnel
//! packet — the same bytes that would otherwise travel as one UDP datagram.
//!
//! The async helpers are gated behind the `tokio` feature so the protocol crate
//! can be used without a Tokio runtime (e.g. pure encode/decode benches).

use std::io;

#[cfg(feature = "tokio")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{FEC_HEADER_SIZE, HEADER_SIZE};

/// Size in bytes of the big-endian frame length prefix.
pub const FRAME_HEADER_SIZE: usize = 4;

/// Maximum frame size: length prefix + largest possible tunnel packet
/// (tunnel header + FEC header + 2048-byte receive buffer).
pub const MAX_FRAME_SIZE: usize = HEADER_SIZE + FEC_HEADER_SIZE + 2048;

/// Decode a big-endian u32 frame length from its 4-byte prefix.
pub fn decode_frame_len(b: &[u8; 4]) -> usize {
    u32::from_be_bytes(*b) as usize
}

/// Read one length-prefixed frame into `buf`.
///
/// Returns `Ok(Some(n))` with the frame payload written to `buf[..n]`, or
/// `Ok(None)` on a clean EOF before any frame bytes.  A frame length of zero
/// or larger than [`MAX_FRAME_SIZE`] is rejected with
/// [`io::ErrorKind::InvalidData`] *before* resizing the buffer, which prevents
/// an attacker-supplied `u32::MAX` length from triggering a huge allocation.
#[cfg(feature = "tokio")]
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    buf: &mut Vec<u8>,
) -> io::Result<Option<usize>> {
    let mut header = [0u8; FRAME_HEADER_SIZE];
    match r.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let n = decode_frame_len(&header);
    if n == 0 || n > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid frame length {n} (max {MAX_FRAME_SIZE})"),
        ));
    }

    buf.resize(n, 0);
    r.read_exact(buf).await?;
    Ok(Some(n))
}

/// Write one length-prefixed frame carrying `payload`.
#[cfg(feature = "tokio")]
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(payload).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_frame_len_roundtrip() {
        for n in [0usize, 1, 255, 256, 65535, 2_000_000] {
            let bytes = (n as u32).to_be_bytes();
            assert_eq!(decode_frame_len(&bytes), n);
        }
    }

    #[test]
    fn test_max_frame_size() {
        assert_eq!(MAX_FRAME_SIZE, HEADER_SIZE + FEC_HEADER_SIZE + 2048);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_frame_roundtrip() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let payload = b"hello tcp frame";
        write_frame(&mut client, payload).await.unwrap();

        let mut buf = Vec::new();
        let n = read_frame(&mut server, &mut buf).await.unwrap().unwrap();
        assert_eq!(n, payload.len());
        assert_eq!(&buf, payload);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_zero_length_rejected() {
        let mut reader: &[u8] = &[0, 0, 0, 0];
        let mut buf = Vec::new();
        let err = read_frame(&mut reader, &mut buf).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_oversized_length_rejected() {
        let len = (MAX_FRAME_SIZE as u32 + 1).to_be_bytes();
        let mut reader: &[u8] = &len;
        let mut buf = Vec::new();
        let err = read_frame(&mut reader, &mut buf).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_eof_returns_none() {
        let mut reader: &[u8] = &[];
        let mut buf = Vec::new();
        let result = read_frame(&mut reader, &mut buf).await.unwrap();
        assert_eq!(result, None);
    }
}
