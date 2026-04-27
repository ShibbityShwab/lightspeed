//! `--live-test` mode: comprehensive live integration test against real proxies.
//!
//! Tests five phases in sequence:
//! 1. Proxy health check (keepalive probe)
//! 2. Route selection (auto-select best proxy)
//! 3. Keepalive echo (10 packets per proxy, latency stats)
//! 4. Data relay (requires `--echo-server`)
//! 5. FEC relay (requires `--echo-server` + `--fec`)

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use tracing::{info, warn};

use crate::cli::parse_proxy_addr;
use crate::config;
use crate::modes::proxy_probe::probe_all_proxies;
use crate::route::selector::NearestSelector;
use crate::route::{ProxyHealth, ProxyNode, RouteSelector};

/// Run the comprehensive live integration test.
pub async fn run_live_test(
    config: &config::Config,
    explicit_proxy: Option<SocketAddrV4>,
    echo_server: Option<SocketAddrV4>,
    fec_enabled: bool,
    fec_k: u8,
) -> anyhow::Result<()> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    info!("🧪 LightSpeed Live Integration Test");
    info!("══════════════════════════════════════════════════════");

    let servers = &config.proxy.servers;
    let data_port = config.proxy.data_port;

    // Build list of proxies to test
    let proxy_addrs: Vec<(String, SocketAddrV4)> = if !servers.is_empty() {
        servers
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                parse_proxy_addr(s).ok().map(|addr| {
                    let addr = if !s.contains(':') {
                        SocketAddrV4::new(*addr.ip(), data_port)
                    } else {
                        addr
                    };
                    (format!("proxy-{}", i), addr)
                })
            })
            .collect()
    } else if let Some(addr) = explicit_proxy {
        vec![("proxy-0".into(), addr)]
    } else {
        anyhow::bail!("No proxies configured. Add servers to config or use --proxy");
    };

    if proxy_addrs.is_empty() {
        anyhow::bail!("No proxy addresses could be resolved");
    }

    let mut total_pass = 0u32;
    let mut total_fail = 0u32;
    let mut total_skip = 0u32;

    // ── Phase 1: Proxy Health Check ─────────────────────────────
    info!("\n📡 Phase 1: Proxy Health Check");
    info!("──────────────────────────────────────────────────────");

    let nodes = probe_all_proxies(
        &proxy_addrs
            .iter()
            .map(|(_, a)| a.to_string())
            .collect::<Vec<_>>(),
        data_port,
    )
    .await;

    let mut healthy_nodes: Vec<&ProxyNode> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let label = if i < proxy_addrs.len() {
            &proxy_addrs[i].0
        } else {
            &node.id
        };
        match node.health {
            ProxyHealth::Healthy => {
                let ms = node.latency_us.unwrap_or(0) as f64 / 1000.0;
                info!(
                    "  ✅ {} ({}) — {:.1}ms [Healthy]",
                    label, node.data_addr, ms
                );
                healthy_nodes.push(node);
                total_pass += 1;
            }
            ProxyHealth::Degraded => {
                let ms = node.latency_us.unwrap_or(0) as f64 / 1000.0;
                warn!(
                    "  ⚠️  {} ({}) — {:.1}ms [Degraded]",
                    label, node.data_addr, ms
                );
                healthy_nodes.push(node);
                total_pass += 1;
            }
            _ => {
                warn!("  ❌ {} ({}) — TIMEOUT [Unhealthy]", label, node.data_addr);
                total_fail += 1;
            }
        }
    }

    if healthy_nodes.is_empty() {
        info!("\n❌ All proxies unreachable — cannot continue");
        info!("══════════════════════════════════════════════════════");
        return Ok(());
    }

    // ── Phase 2: Route Selection ────────────────────────────────
    info!("\n🔀 Phase 2: Route Selection");
    info!("──────────────────────────────────────────────────────");

    if nodes.len() >= 2 {
        let dummy_gs = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0);
        let selector = NearestSelector::new();
        match selector.select(dummy_gs, &nodes) {
            Ok(route) => {
                let ms = route.primary.latency_us.unwrap_or(0) as f64 / 1000.0;
                info!("  Strategy:  {:?}", route.strategy);
                info!("  Selected:  {} ({:.1}ms)", route.primary.id, ms);
                if !route.backups.is_empty() {
                    let backups: Vec<String> = route
                        .backups
                        .iter()
                        .map(|b| {
                            format!(
                                "{} ({:.1}ms)",
                                b.id,
                                b.latency_us.unwrap_or(0) as f64 / 1000.0
                            )
                        })
                        .collect();
                    info!("  Backups:   {}", backups.join(", "));
                }
                info!("  ✅ Route selection working");
                total_pass += 1;
            }
            Err(e) => {
                warn!("  ❌ Route selection failed: {}", e);
                total_fail += 1;
            }
        }
    } else {
        info!("  ⏭️  Only 1 proxy — route selection not applicable");
        total_skip += 1;
    }

    // ── Phase 3: Keepalive Echo (detailed) ──────────────────────
    info!("\n💓 Phase 3: Keepalive Echo (10 packets each)");
    info!("──────────────────────────────────────────────────────");

    for (label, addr) in proxy_addrs.iter() {
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                warn!("  ❌ {} — socket bind failed: {}", label, e);
                total_fail += 1;
                continue;
            }
        };

        let num_pings = 10;
        let mut rtts = Vec::with_capacity(num_pings);

        for seq in 0..num_pings as u16 {
            let send_time = std::time::Instant::now();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u32;

            let header = TunnelHeader::keepalive(seq, ts);
            let packet = header.encode_to_array();

            if socket.send_to(&packet, addr).await.is_err() {
                continue;
            }

            let mut buf = vec![0u8; 128];
            if let Ok(Ok((len, _))) =
                tokio::time::timeout(Duration::from_millis(3000), socket.recv_from(&mut buf)).await
            {
                if TunnelHeader::decode(&buf[..len]).is_ok() {
                    rtts.push(send_time.elapsed().as_micros() as u64);
                }
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if rtts.is_empty() {
            warn!("  ❌ {} — 0/{} keepalives echoed", label, num_pings);
            total_fail += 1;
        } else {
            rtts.sort();
            let avg = rtts.iter().sum::<u64>() as f64 / rtts.len() as f64 / 1000.0;
            let min = *rtts.first().unwrap() as f64 / 1000.0;
            let max = *rtts.last().unwrap() as f64 / 1000.0;
            let jitter = if rtts.len() > 1 {
                let diffs: Vec<f64> = rtts
                    .windows(2)
                    .map(|w| (w[1] as f64 - w[0] as f64).abs() / 1000.0)
                    .collect();
                diffs.iter().sum::<f64>() / diffs.len() as f64
            } else {
                0.0
            };
            info!(
                "  ✅ {}: {}/{} received, avg={:.1}ms, min={:.1}ms, max={:.1}ms, jitter={:.1}ms",
                label,
                rtts.len(),
                num_pings,
                avg,
                min,
                max,
                jitter
            );
            total_pass += 1;
        }
    }

    // ── Phase 4: Data Relay ─────────────────────────────────────
    info!("\n📦 Phase 4: Data Relay");
    info!("──────────────────────────────────────────────────────");

    if let Some(echo_addr) = echo_server {
        for (label, proxy) in &proxy_addrs {
            let socket = match UdpSocket::bind("0.0.0.0:0").await {
                Ok(s) => s,
                Err(e) => {
                    warn!("  ❌ {} — socket bind failed: {}", label, e);
                    total_fail += 1;
                    continue;
                }
            };

            let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
            let num_packets = 5;
            let mut rtts = Vec::new();
            let mut payload_matches = 0u32;

            for seq in 0..num_packets as u16 {
                let payload = format!("LIGHTSPEED_LIVE_TEST_{}", seq);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u32;

                let header = TunnelHeader::new(seq, ts, local_addr, echo_addr);
                let packet = header.encode_with_payload(payload.as_bytes());

                let send_time = std::time::Instant::now();
                if socket.send_to(&packet, proxy).await.is_err() {
                    continue;
                }

                let mut buf = vec![0u8; 2048];
                if let Ok(Ok((len, _))) =
                    tokio::time::timeout(Duration::from_millis(5000), socket.recv_from(&mut buf))
                        .await
                {
                    let rtt = send_time.elapsed().as_micros() as u64;
                    rtts.push(rtt);

                    if let Ok((_hdr, resp_payload)) = TunnelHeader::decode_with_payload(&buf[..len])
                    {
                        if resp_payload == payload.as_bytes() {
                            payload_matches += 1;
                        }
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            if rtts.is_empty() {
                warn!(
                    "  ❌ {} → echo({}): 0/{} responses",
                    label, echo_addr, num_packets
                );
                total_fail += 1;
            } else {
                let avg = rtts.iter().sum::<u64>() as f64 / rtts.len() as f64 / 1000.0;
                let min = *rtts.iter().min().unwrap() as f64 / 1000.0;
                let max = *rtts.iter().max().unwrap() as f64 / 1000.0;
                let status = if payload_matches == rtts.len() as u32 {
                    "✅"
                } else {
                    "⚠️"
                };
                info!(
                    "  {} {} → echo({}): {}/{} received, {}/{} payload match, avg={:.1}ms min={:.1}ms max={:.1}ms",
                    status, label, echo_addr,
                    rtts.len(), num_packets,
                    payload_matches, rtts.len(),
                    avg, min, max
                );
                if payload_matches > 0 {
                    total_pass += 1;
                } else {
                    total_fail += 1;
                }
            }
        }
    } else {
        info!("  ⏭️  Skipped — no echo server configured");
        info!("     Use --echo-server <ip:port> to test data relay");
        info!("     (Run tools/echo_server.py on a Vultr node first)");
        total_skip += 1;
    }

    // ── Phase 5: FEC Relay ──────────────────────────────────────
    info!("\n🔧 Phase 5: FEC Relay");
    info!("──────────────────────────────────────────────────────");

    if let Some(echo_addr) = echo_server {
        if fec_enabled {
            use bytes::BytesMut;
            use lightspeed_protocol::{FecEncoder, FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};

            for (label, proxy) in &proxy_addrs {
                let socket = match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("  ❌ {} — socket bind failed: {}", label, e);
                        total_fail += 1;
                        continue;
                    }
                };

                let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
                let mut encoder = FecEncoder::new(fec_k);
                let mut responses = 0u32;
                let num_data_packets = fec_k as u16 * 2; // 2 full FEC blocks

                for seq in 0..num_data_packets {
                    let payload = format!("FEC_TEST_{}_{}", label, seq);
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;

                    let block_id = encoder.block_id();
                    let index = encoder.current_index();

                    let header = TunnelHeader::new_fec(seq, ts, local_addr, echo_addr);
                    let fec_hdr = FecHeader::data(block_id, index, fec_k);

                    let mut pkt =
                        BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
                    pkt.extend_from_slice(&header.encode_to_array());
                    fec_hdr.encode(&mut pkt);
                    pkt.extend_from_slice(payload.as_bytes());

                    let parity = encoder.add_packet(payload.as_bytes());
                    let _ = socket.send_to(&pkt, proxy).await;

                    // Send parity when the block completes
                    if let Some(parity_bytes) = parity {
                        let parity_header =
                            TunnelHeader::new_fec(seq + 1000, ts, local_addr, echo_addr);
                        let parity_fec = FecHeader::parity(block_id, fec_k);
                        let mut parity_pkt = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                        );
                        parity_pkt.extend_from_slice(&parity_header.encode_to_array());
                        parity_fec.encode(&mut parity_pkt);
                        parity_pkt.extend_from_slice(&parity_bytes);
                        let _ = socket.send_to(&parity_pkt, proxy).await;
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;
                }

                // Collect responses
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut buf = vec![0u8; 2048];
                while std::time::Instant::now() < deadline {
                    match tokio::time::timeout(
                        Duration::from_millis(500),
                        socket.recv_from(&mut buf),
                    )
                    .await
                    {
                        Ok(Ok((len, _))) => {
                            if TunnelHeader::decode(&buf[..len]).is_ok() {
                                responses += 1;
                            }
                        }
                        _ => break,
                    }
                }

                let blocks = num_data_packets / fec_k as u16;
                let total_sent = num_data_packets + blocks; // data + parity
                if responses > 0 {
                    info!(
                        "  ✅ {} FEC(K={}): sent {} data + {} parity, received {} responses",
                        label, fec_k, num_data_packets, blocks, responses
                    );
                    total_pass += 1;
                } else {
                    warn!(
                        "  ❌ {} FEC(K={}): sent {} packets, 0 responses",
                        label, fec_k, total_sent
                    );
                    total_fail += 1;
                }
            }
        } else {
            info!("  ⏭️  Skipped — FEC not enabled (use --fec to test)");
            total_skip += 1;
        }
    } else {
        info!("  ⏭️  Skipped — no echo server configured");
        total_skip += 1;
    }

    // ── Summary ─────────────────────────────────────────────────
    info!("\n══════════════════════════════════════════════════════");
    info!("📊 Live Integration Test Summary");
    info!("──────────────────────────────────────────────────────");
    info!("  Proxies tested:     {}", proxy_addrs.len());
    info!(
        "  Healthy:            {}/{}",
        healthy_nodes.len(),
        nodes.len()
    );
    if let Some(best) = nodes
        .iter()
        .filter(|n| n.latency_us.is_some())
        .min_by_key(|n| n.latency_us)
    {
        info!(
            "  Best latency:       {:.1}ms ({})",
            best.latency_us.unwrap() as f64 / 1000.0,
            best.id
        );
    }
    info!("  ────────────────────────────────────────────");
    info!("  ✅ Passed:  {}", total_pass);
    info!("  ❌ Failed:  {}", total_fail);
    info!("  ⏭️  Skipped: {}", total_skip);
    info!("──────────────────────────────────────────────────────");

    if total_fail == 0 {
        info!("  🎉 All tests passed! Live infrastructure verified.");
    } else {
        warn!("  ⚠️  {} test(s) failed — check proxy status", total_fail);
    }

    info!("══════════════════════════════════════════════════════");
    Ok(())
}
