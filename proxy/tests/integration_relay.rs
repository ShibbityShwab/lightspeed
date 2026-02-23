//! # End-to-End Relay Integration Test
//!
//! Tests the full data plane path:
//!   Client → Proxy (decode + forward) → Echo Server → Proxy (re-wrap) → Client
//!
//! Uses a local UDP echo server to simulate a game server.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use lightspeed_protocol::{TunnelHeader, HEADER_SIZE};
use tokio::net::UdpSocket;

/// A simple UDP echo server that sends back whatever it receives.
async fn run_echo_server(socket: Arc<UdpSocket>) {
    let mut buf = vec![0u8; 2048];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                let _ = socket.send_to(&buf[..len], addr).await;
            }
            Err(_) => break,
        }
    }
}

/// Get a timestamp in microseconds.
fn now_us() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

#[tokio::test]
async fn test_keepalive_echo() {
    // This test verifies that the proxy echoes keepalive packets.
    //
    // We simulate this at the protocol level: send a keepalive-encoded
    // packet and verify decode works correctly.

    let seq = 42;
    let ts = now_us();
    let keepalive = TunnelHeader::keepalive(seq, ts);
    assert!(keepalive.is_keepalive());

    let encoded = keepalive.encode();
    assert_eq!(encoded.len(), HEADER_SIZE);

    let decoded = TunnelHeader::decode(&encoded).unwrap();
    assert_eq!(decoded.sequence, seq);
    assert!(decoded.is_keepalive());
}

#[tokio::test]
async fn test_full_tunnel_round_trip() {
    // This test verifies the full encode → decode → forward → echo → re-wrap → decode path
    // without actually running the relay server (pure protocol-level test).

    let client_addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let game_payload = b"game state update packet data";

    // Step 1: Client creates tunnel packet
    let client_header = TunnelHeader::new(1, now_us(), client_addr, game_server);
    let tunnel_packet = client_header.encode_with_payload(game_payload);

    // Step 2: Proxy decodes tunnel packet
    let (decoded_header, decoded_payload) =
        TunnelHeader::decode_with_payload(&tunnel_packet).unwrap();
    assert_eq!(decoded_header.orig_src_addr(), client_addr);
    assert_eq!(decoded_header.orig_dst_addr(), game_server);
    assert_eq!(decoded_payload, game_payload);

    // Step 3: Proxy forwards raw payload to game server (simulated)
    // Game server would receive just `game_payload` at `game_server` address

    // Step 4: Game server echoes back the same payload (simulated)
    let echo_payload = decoded_payload; // Same data comes back

    // Step 5: Proxy re-wraps the response
    let response_header = TunnelHeader::new(
        1,
        now_us(),
        game_server, // source is now the game server
        client_addr, // destination is the original client
    );
    let response_packet = response_header.encode_with_payload(echo_payload);

    // Step 6: Client decodes the response
    let (final_header, final_payload) =
        TunnelHeader::decode_with_payload(&response_packet).unwrap();
    assert_eq!(final_header.orig_src_addr(), game_server);
    assert_eq!(final_header.orig_dst_addr(), client_addr);
    assert_eq!(final_payload, game_payload);
}

#[tokio::test]
async fn test_udp_relay_with_echo_server() {
    // End-to-end test with actual UDP sockets:
    //   Client socket → Proxy socket → Echo server socket → Proxy → Client

    // Bind sockets
    let echo_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let echo_addr = echo_socket.local_addr().unwrap();
    let echo_addr_v4 = match echo_addr {
        std::net::SocketAddr::V4(v4) => v4,
        _ => panic!("Expected IPv4"),
    };

    let proxy_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let proxy_addr = proxy_socket.local_addr().unwrap();
    let proxy_addr_v4 = match proxy_addr {
        std::net::SocketAddr::V4(v4) => v4,
        _ => panic!("Expected IPv4"),
    };

    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = client_socket.local_addr().unwrap();
    let client_addr_v4 = match client_addr {
        std::net::SocketAddr::V4(v4) => v4,
        _ => panic!("Expected IPv4"),
    };

    // Start echo server
    let echo_handle = tokio::spawn({
        let echo_socket = Arc::clone(&echo_socket);
        async move { run_echo_server(echo_socket).await }
    });

    // Start a simple proxy relay task
    let proxy_handle = tokio::spawn({
        let proxy_socket = Arc::clone(&proxy_socket);
        async move {
            let mut buf = vec![0u8; 2048];

            // Receive from client, forward to echo, receive echo, send back to client
            for _ in 0..3 {
                // 1. Receive tunnel packet from client
                let (len, from_addr) = proxy_socket.recv_from(&mut buf).await.unwrap();
                let from_v4 = match from_addr {
                    std::net::SocketAddr::V4(v4) => v4,
                    _ => panic!("Expected IPv4"),
                };

                // 2. Decode header
                let (header, payload) = TunnelHeader::decode_with_payload(&buf[..len]).unwrap();

                if header.is_keepalive() {
                    // Echo keepalive back
                    let resp = TunnelHeader::keepalive(header.sequence, now_us());
                    let _ = proxy_socket.send_to(&resp.encode(), from_addr).await;
                    continue;
                }

                // 3. Forward raw payload to game server (echo)
                let game_dest = header.orig_dst_addr();
                let outbound = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                outbound.send_to(payload, game_dest).await.unwrap();

                // 4. Receive echo response
                let (echo_len, _) = outbound.recv_from(&mut buf).await.unwrap();

                // 5. Re-wrap and send back to client
                let resp_header = TunnelHeader::new(header.sequence, now_us(), game_dest, from_v4);
                let resp_packet = resp_header.encode_with_payload(&buf[..echo_len]);
                proxy_socket.send_to(&resp_packet, from_addr).await.unwrap();
            }
        }
    });

    // Client sends tunnel packets and verifies responses
    let game_data = b"test game packet payload";

    for i in 0..3u16 {
        // Create tunnel packet
        let header = TunnelHeader::new(i, now_us(), client_addr_v4, echo_addr_v4);
        let packet = header.encode_with_payload(game_data);

        // Send to proxy
        client_socket.send_to(&packet, proxy_addr).await.unwrap();

        // Receive response
        let mut buf = vec![0u8; 2048];
        let result =
            tokio::time::timeout(Duration::from_secs(2), client_socket.recv_from(&mut buf)).await;

        match result {
            Ok(Ok((len, _from))) => {
                let (resp_header, resp_payload) =
                    TunnelHeader::decode_with_payload(&buf[..len]).unwrap();

                // Verify the payload came back intact
                assert_eq!(resp_payload, game_data, "Payload mismatch on packet {}", i);
                assert_eq!(resp_header.sequence, i);
                assert_eq!(resp_header.orig_src_addr(), echo_addr_v4);
                assert_eq!(resp_header.orig_dst_addr(), client_addr_v4);
            }
            Ok(Err(e)) => panic!("Recv error on packet {}: {}", i, e),
            Err(_) => panic!("Timeout waiting for response on packet {}", i),
        }
    }

    // Clean up
    echo_handle.abort();
    proxy_handle.abort();
}
