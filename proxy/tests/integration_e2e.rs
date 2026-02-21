//! # End-to-End Integration Tests
//!
//! Tests the full tunnel lifecycle: client → proxy → game server → proxy → client.
//! Uses a manual proxy relay and UDP echo server for full round-trip verification.
//!
//! Test coverage:
//! - Multi-packet relay correctness
//! - Multiple concurrent client sessions
//! - Keepalive round-trip
//! - FIN (graceful close) handling
//! - Large payload (near MTU) relay
//! - Burst traffic handling
//! - Sequence number preservation

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use lightspeed_protocol::{flags, TunnelHeader};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

// ── Helpers ─────────────────────────────────────────────────────────

fn now_us() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

fn to_v4(addr: SocketAddr) -> SocketAddrV4 {
    match addr {
        SocketAddr::V4(v4) => v4,
        _ => panic!("Expected IPv4"),
    }
}

/// Spawn a UDP echo server that reflects all received data.
async fn spawn_echo_server() -> (SocketAddrV4, JoinHandle<()>) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = to_v4(socket.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, from)) => {
                    let _ = socket.send_to(&buf[..len], from).await;
                }
                Err(_) => break,
            }
        }
    });
    (addr, handle)
}

/// Spawn a manual proxy relay that decodes tunnel packets, forwards to echo,
/// re-wraps responses, and sends back to the client. Runs indefinitely.
async fn spawn_manual_proxy(echo_addr: SocketAddrV4) -> (SocketAddrV4, JoinHandle<()>) {
    let proxy_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = to_v4(proxy_socket.local_addr().unwrap());

    let handle = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            let (len, client_addr) = match proxy_socket.recv_from(&mut buf).await {
                Ok(r) => r,
                Err(_) => break,
            };

            let client_v4 = to_v4(client_addr);

            let (header, payload) = match TunnelHeader::decode_with_payload(&buf[..len]) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Handle keepalive
            if header.is_keepalive() {
                let resp = TunnelHeader::keepalive(header.sequence, now_us())
                    .with_session_token(header.session_token);
                let _ = proxy_socket.send_to(&resp.encode(), client_addr).await;
                continue;
            }

            // Handle FIN
            if header.is_fin() {
                // Acknowledge by echoing FIN back
                let mut fin_resp = TunnelHeader::keepalive(header.sequence, now_us());
                fin_resp.flags = flags::FIN;
                let _ = proxy_socket.send_to(&fin_resp.encode(), client_addr).await;
                continue;
            }

            // Forward payload to echo server
            let outbound = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            if outbound.send_to(payload, echo_addr).await.is_err() {
                continue;
            }

            let mut echo_buf = vec![0u8; 2048];
            let echo_result = tokio::time::timeout(
                Duration::from_secs(2),
                outbound.recv_from(&mut echo_buf),
            )
            .await;

            if let Ok(Ok((echo_len, _))) = echo_result {
                let resp_header = TunnelHeader::new(
                    header.sequence,
                    now_us(),
                    header.orig_dst_addr(),
                    client_v4,
                )
                .with_session_token(header.session_token);

                let resp_packet = resp_header.encode_with_payload(&echo_buf[..echo_len]);
                let _ = proxy_socket.send_to(&resp_packet, client_addr).await;
            }
        }
    });

    (addr, handle)
}

/// Send a tunnel packet and wait for response.
async fn send_and_recv(
    client: &UdpSocket,
    proxy_addr: SocketAddrV4,
    header: &TunnelHeader,
    payload: &[u8],
    timeout_ms: u64,
) -> Option<(TunnelHeader, Vec<u8>)> {
    let packet = header.encode_with_payload(payload);
    client
        .send_to(&packet, proxy_addr)
        .await
        .expect("send failed");

    let mut buf = vec![0u8; 2048];
    match tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        client.recv_from(&mut buf),
    )
    .await
    {
        Ok(Ok((len, _))) => {
            let (h, p) = TunnelHeader::decode_with_payload(&buf[..len]).unwrap();
            Some((h, p.to_vec()))
        }
        _ => None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_multi_packet_relay() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

    // Send 10 packets, verify all come back correctly
    for seq in 0..10u16 {
        let payload = format!("game_packet_{}", seq);
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server)
            .with_session_token(42);

        let result = send_and_recv(&client, proxy_addr, &header, payload.as_bytes(), 2000).await;
        let (resp_h, resp_p) = result.unwrap_or_else(|| panic!("No response for packet {}", seq));

        assert_eq!(resp_p, payload.as_bytes(), "Payload mismatch on packet {}", seq);
        assert_eq!(resp_h.sequence, seq, "Sequence mismatch on packet {}", seq);
        assert_eq!(resp_h.session_token, 42, "Token mismatch on packet {}", seq);
    }

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_concurrent_clients() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;

    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let num_clients = 5;

    let mut handles = Vec::new();

    for client_id in 0..num_clients {
        let handle = tokio::spawn(async move {
            let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client_addr = to_v4(client.local_addr().unwrap());

            for seq in 0..5u16 {
                let payload = format!("client{}_{}", client_id, seq);
                let header = TunnelHeader::new(seq, now_us(), client_addr, game_server)
                    .with_session_token(client_id as u8);

                let result =
                    send_and_recv(&client, proxy_addr, &header, payload.as_bytes(), 3000).await;
                let (_, resp_p) =
                    result.unwrap_or_else(|| panic!("Client {} packet {} timeout", client_id, seq));
                assert_eq!(resp_p, payload.as_bytes());
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.await.unwrap();
    }

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_keepalive_round_trip() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    // Send 5 keepalives, verify all echoed
    for seq in 0..5u16 {
        let keepalive = TunnelHeader::keepalive(seq, now_us()).with_session_token(10);
        let packet = keepalive.encode();
        client.send_to(&packet, proxy_addr).await.unwrap();

        let mut buf = vec![0u8; 2048];
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            client.recv_from(&mut buf),
        )
        .await;

        match result {
            Ok(Ok((len, _))) => {
                let resp = TunnelHeader::decode(&buf[..len]).unwrap();
                assert!(resp.is_keepalive(), "Response should be keepalive");
                assert_eq!(resp.sequence, seq, "Keepalive seq mismatch");
                assert_eq!(resp.session_token, 10, "Token should be preserved");
            }
            _ => panic!("Keepalive {} timeout", seq),
        }
    }

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_fin_packet_handling() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

    // First send a normal packet to establish "session"
    let header = TunnelHeader::new(1, now_us(), client_addr, game_server);
    let result = send_and_recv(&client, proxy_addr, &header, b"hello", 2000).await;
    assert!(result.is_some(), "Normal packet should get response");

    // Now send FIN
    let mut fin_header = TunnelHeader::new(2, now_us(), client_addr, game_server);
    fin_header.flags = flags::FIN;
    let fin_packet = fin_header.encode();
    client.send_to(&fin_packet, proxy_addr).await.unwrap();

    // Should get FIN ack back
    let mut buf = vec![0u8; 2048];
    let result = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf)).await;
    match result {
        Ok(Ok((len, _))) => {
            let resp = TunnelHeader::decode(&buf[..len]).unwrap();
            assert!(resp.is_fin(), "Response should have FIN flag");
        }
        _ => panic!("FIN ack timeout"),
    }

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_large_payload_near_mtu() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

    // Test various payload sizes up to near-MTU
    let sizes = [1, 100, 500, 1000, 1300];

    for &size in &sizes {
        let payload: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let header = TunnelHeader::new(size as u16, now_us(), client_addr, game_server);

        let result = send_and_recv(&client, proxy_addr, &header, &payload, 2000).await;
        let (_, resp_p) =
            result.unwrap_or_else(|| panic!("No response for {}-byte payload", size));
        assert_eq!(resp_p.len(), size, "Payload size mismatch for {}-byte test", size);
        assert_eq!(resp_p, payload, "Payload content mismatch for {}-byte test", size);
    }

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_burst_traffic() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

    let burst_count = 50u16;

    // Send all packets as fast as possible
    for seq in 0..burst_count {
        let payload = format!("burst_{:04}", seq);
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let packet = header.encode_with_payload(payload.as_bytes());
        client.send_to(&packet, proxy_addr).await.unwrap();
    }

    // Collect responses (some might be dropped under load, that's OK)
    let mut received = 0u16;
    let mut buf = vec![0u8; 2048];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, client.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                let (_, payload) = TunnelHeader::decode_with_payload(&buf[..len]).unwrap();
                assert!(payload.starts_with(b"burst_"), "Unexpected payload");
                received += 1;
                if received >= burst_count {
                    break;
                }
            }
            _ => break,
        }
    }

    // At least 80% should make it through (generous threshold for CI)
    let min_expected = (burst_count as f64 * 0.8) as u16;
    assert!(
        received >= min_expected,
        "Only received {}/{} burst packets (min {})",
        received,
        burst_count,
        min_expected
    );

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_sequence_number_preservation() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

    // Test specific sequence numbers including edge cases
    let sequences = [0, 1, 255, 1000, u16::MAX - 1, u16::MAX];

    for &seq in &sequences {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let result = send_and_recv(&client, proxy_addr, &header, b"seq_test", 2000).await;
        let (resp_h, _) =
            result.unwrap_or_else(|| panic!("No response for seq={}", seq));
        assert_eq!(resp_h.sequence, seq, "Sequence not preserved for {}", seq);
    }

    echo_h.abort();
    proxy_h.abort();
}
