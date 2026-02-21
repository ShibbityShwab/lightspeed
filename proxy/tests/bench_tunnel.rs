//! # Tunnel Performance Benchmarks
//!
//! Measures key performance characteristics of the LightSpeed tunnel:
//! - Latency overhead (encode → proxy → decode round-trip)
//! - Throughput (sustained packet rate)
//! - Latency percentiles (p50, p95, p99)
//!
//! These are integration-level benchmarks using actual UDP sockets on localhost.
//! Results are printed to stdout for the test report.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lightspeed_protocol::TunnelHeader;
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
            if header.is_keepalive() {
                let resp = TunnelHeader::keepalive(header.sequence, now_us());
                let _ = proxy_socket.send_to(&resp.encode(), client_addr).await;
                continue;
            }

            let outbound = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            if outbound.send_to(payload, echo_addr).await.is_err() {
                continue;
            }
            let mut echo_buf = vec![0u8; 2048];
            if let Ok(Ok((echo_len, _))) = tokio::time::timeout(
                Duration::from_secs(2),
                outbound.recv_from(&mut echo_buf),
            )
            .await
            {
                let resp = TunnelHeader::new(
                    header.sequence,
                    now_us(),
                    header.orig_dst_addr(),
                    client_v4,
                );
                let pkt = resp.encode_with_payload(&echo_buf[..echo_len]);
                let _ = proxy_socket.send_to(&pkt, client_addr).await;
            }
        }
    });
    (addr, handle)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * sorted.len() as f64 - 1.0).max(0.0) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── Benchmarks ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_latency_overhead() {
    // Measure round-trip latency through the tunnel vs. raw UDP echo.

    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let payload = b"benchmark payload data 64 bytes padded with filler!!!!!!!!!!!!";

    // ── Phase 1: Measure raw UDP echo latency (baseline) ──

    let raw_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut raw_latencies = Vec::with_capacity(100);

    // Warmup
    for _ in 0..10 {
        raw_client.send_to(payload, echo_addr).await.unwrap();
        let mut buf = [0u8; 128];
        let _ = tokio::time::timeout(Duration::from_secs(1), raw_client.recv_from(&mut buf)).await;
    }

    for _ in 0..100 {
        let start = Instant::now();
        raw_client.send_to(payload, echo_addr).await.unwrap();
        let mut buf = [0u8; 128];
        if tokio::time::timeout(Duration::from_secs(1), raw_client.recv_from(&mut buf))
            .await
            .is_ok()
        {
            raw_latencies.push(start.elapsed().as_micros() as f64);
        }
    }

    // ── Phase 2: Measure tunneled latency ──

    let mut tunnel_latencies = Vec::with_capacity(100);

    // Warmup
    for seq in 0..10u16 {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let pkt = header.encode_with_payload(payload);
        client.send_to(&pkt, proxy_addr).await.unwrap();
        let mut buf = [0u8; 256];
        let _ = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf)).await;
    }

    for seq in 0..100u16 {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let pkt = header.encode_with_payload(payload);

        let start = Instant::now();
        client.send_to(&pkt, proxy_addr).await.unwrap();
        let mut buf = [0u8; 256];
        if tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf))
            .await
            .is_ok()
        {
            tunnel_latencies.push(start.elapsed().as_micros() as f64);
        }
    }

    // ── Results ──

    raw_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tunnel_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let raw_p50 = percentile(&raw_latencies, 50.0);
    let tunnel_p50 = percentile(&tunnel_latencies, 50.0);
    let overhead_us = tunnel_p50 - raw_p50;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║       LATENCY OVERHEAD BENCHMARK         ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║ Raw UDP echo p50:    {:>8.0} μs          ║", raw_p50);
    println!("║ Tunneled p50:        {:>8.0} μs          ║", tunnel_p50);
    println!("║ Overhead:            {:>8.0} μs          ║", overhead_us);
    println!("║ Samples: {} raw, {} tunneled      ║", raw_latencies.len(), tunnel_latencies.len());
    println!("╚══════════════════════════════════════════╝\n");

    // Overhead should be reasonable (< 5ms = 5000 μs on localhost)
    assert!(
        overhead_us < 5000.0,
        "Tunnel overhead {:.0}μs exceeds 5ms target",
        overhead_us
    );

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_throughput_measurement() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let payload = b"throughput test payload data";

    let total_packets = 200u16;

    // Send all packets as fast as possible
    let send_start = Instant::now();
    for seq in 0..total_packets {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let pkt = header.encode_with_payload(payload);
        client.send_to(&pkt, proxy_addr).await.unwrap();
    }
    let send_elapsed = send_start.elapsed();

    // Receive responses
    let mut received = 0u32;
    let mut buf = [0u8; 256];
    let recv_start = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, client.recv_from(&mut buf)).await {
            Ok(Ok(_)) => {
                received += 1;
                if received >= total_packets as u32 {
                    break;
                }
            }
            _ => break,
        }
    }
    let recv_elapsed = recv_start.elapsed();

    let send_pps = total_packets as f64 / send_elapsed.as_secs_f64();
    let recv_pps = received as f64 / recv_elapsed.as_secs_f64();
    let loss_pct = (1.0 - received as f64 / total_packets as f64) * 100.0;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║       THROUGHPUT BENCHMARK               ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║ Packets sent:     {:>6}                 ║", total_packets);
    println!("║ Packets received: {:>6}                 ║", received);
    println!("║ Send rate:      {:>8.0} pps             ║", send_pps);
    println!("║ Recv rate:      {:>8.0} pps             ║", recv_pps);
    println!("║ Packet loss:    {:>7.1}%                ║", loss_pct);
    println!("╚══════════════════════════════════════════╝\n");

    // At least 70% of packets should be received
    assert!(
        received as f64 >= total_packets as f64 * 0.7,
        "Too much packet loss: {}/{} received",
        received,
        total_packets
    );

    echo_h.abort();
    proxy_h.abort();
}

#[tokio::test]
async fn test_latency_percentiles() {
    let (echo_addr, echo_h) = spawn_echo_server().await;
    let (proxy_addr, proxy_h) = spawn_manual_proxy(echo_addr).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());
    let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
    let payload = b"latency percentile test data";

    let mut latencies = Vec::with_capacity(200);

    // Warmup
    for seq in 0..20u16 {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let pkt = header.encode_with_payload(payload);
        client.send_to(&pkt, proxy_addr).await.unwrap();
        let mut buf = [0u8; 256];
        let _ = tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut buf)).await;
    }

    // Measure
    for seq in 0..200u16 {
        let header = TunnelHeader::new(seq, now_us(), client_addr, game_server);
        let pkt = header.encode_with_payload(payload);

        let start = Instant::now();
        client.send_to(&pkt, proxy_addr).await.unwrap();
        let mut buf = [0u8; 256];
        match tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf)).await {
            Ok(Ok(_)) => {
                latencies.push(start.elapsed().as_micros() as f64);
            }
            _ => {} // Skip timeouts
        }
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = percentile(&latencies, 50.0);
    let p95 = percentile(&latencies, 95.0);
    let p99 = percentile(&latencies, 99.0);
    let min = latencies.first().copied().unwrap_or(0.0);
    let max = latencies.last().copied().unwrap_or(0.0);
    let avg = latencies.iter().sum::<f64>() / latencies.len().max(1) as f64;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║     LATENCY PERCENTILES (μs)             ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║ Samples:  {:>6}                          ║", latencies.len());
    println!("║ Min:     {:>8.0}                         ║", min);
    println!("║ Avg:     {:>8.0}                         ║", avg);
    println!("║ p50:     {:>8.0}                         ║", p50);
    println!("║ p95:     {:>8.0}                         ║", p95);
    println!("║ p99:     {:>8.0}                         ║", p99);
    println!("║ Max:     {:>8.0}                         ║", max);
    println!("╚══════════════════════════════════════════╝\n");

    // p99 should be under 10ms on localhost
    assert!(
        p99 < 10_000.0,
        "p99 latency {:.0}μs exceeds 10ms",
        p99
    );

    // Should have received most packets
    assert!(
        latencies.len() >= 150,
        "Too many dropped packets: only {} of 200 received",
        latencies.len()
    );

    echo_h.abort();
    proxy_h.abort();
}
