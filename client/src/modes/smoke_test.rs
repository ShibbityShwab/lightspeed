//! `--smoke-test` mode: synthetic end-to-end validation of the Linux
//! interceptor.
//!
//! The test renames its own process to a Fortnite executable, opens a UDP
//! socket to a TEST-NET address (public-looking, never routed), and drives the
//! real nftables REDIRECT path through an in-process mock proxy. It proves, in
//! one run: waiting state, exact-IP rule install, kernel REDIRECT, tunnel
//! header destination, reply injection with conntrack de-NAT, rotation without
//! misrouting, rule teardown, and a non-spinning receive loop.
//!
//! Requires root (CAP_NET_ADMIN) and `nft`; otherwise it reports why and exits
//! successfully.

use std::net::SocketAddrV4;
#[cfg(target_os = "linux")]
use std::time::Duration;

pub async fn run_smoke_test(proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = proxy_addr;
        tracing::info!("ℹ️  Synthetic E2E smoke test is Linux-only; skipping.");
        tracing::info!("✅ Smoke test PASSED (skipped on this platform)");
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        run_smoke_test_linux(proxy_addr).await
    }
}

#[cfg(target_os = "linux")]
async fn run_smoke_test_linux(proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
    use std::net::Ipv4Addr;
    use std::sync::{Arc, Mutex};

    use crate::interceptor::rotation::ROTATE_SCAN_INTERVAL;

    tracing::info!("🔥 LightSpeed Smoke Test (synthetic E2E)");

    if !is_root() {
        tracing::info!("ℹ️  Skipping: requires root (CAP_NET_ADMIN) for nftables REDIRECT.");
        tracing::info!("✅ Smoke test PASSED (skipped — not root)");
        return Ok(());
    }
    if !command_available("nft") {
        tracing::info!("ℹ️  Skipping: `nft` was not found in PATH.");
        tracing::info!("✅ Smoke test PASSED (skipped — nft unavailable)");
        return Ok(());
    }

    const PAYLOAD: &[u8] = b"ls-smoke-payload";
    const N: usize = 5;
    let target_a = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 34568);
    let target_b = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 10), 34568);

    remove_all_lightspeed_tables();

    // ── In-process mock proxy ─────────────────────────────────────────
    let mock_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    let mock_addr = match mock_socket.local_addr()? {
        std::net::SocketAddr::V4(v4) => v4,
        std::net::SocketAddr::V6(_) => anyhow::bail!("mock proxy bound IPv6"),
    };
    tracing::info!("🔌 Mock proxy: {mock_addr}");
    let received: Arc<Mutex<Vec<SocketAddrV4>>> = Arc::new(Mutex::new(Vec::new()));
    let received_task = Arc::clone(&received);
    let mock_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        while let Ok((len, from)) = mock_socket.recv_from(&mut buf).await {
            let Ok((header, payload)) =
                lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len])
            else {
                continue;
            };
            if header.is_keepalive() {
                continue;
            }
            received_task.lock().unwrap().push(header.orig_dst_addr());
            let reply_header = header.make_response(header.sequence, header.timestamp_us);
            let reply = reply_header.encode_with_payload(payload);
            let _ = mock_socket.send_to(&reply, from).await;
        }
    });

    // Route the interceptor's data plane at the mock, and clear any multipath.
    crate::session::set_multipath_paths(Vec::new());
    crate::session::set_current_proxy(mock_addr);

    // ── Synthetic game process + connected UDP socket ─────────────────
    set_process_name(b"FortniteClient-Win64-Shipping.exe");
    let game = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    game.connect(target_a).await?;
    let game_port = match game.local_addr()? {
        std::net::SocketAddr::V4(v4) => v4.port(),
        std::net::SocketAddr::V6(_) => anyhow::bail!("game socket bound IPv6"),
    };
    tracing::info!("🎮 Synthetic game socket: 0.0.0.0:{game_port} → {target_a}");

    // ── Start interceptor: Fortnite is dynamic, so it starts unlocked ──
    let interceptor = crate::interceptor::create_interceptor();
    interceptor
        .check_availability()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let config = crate::interceptor::InterceptorConfig {
        game_name: "Fortnite".into(),
        pid: None,
        port_range: (1024, 65535),
        initial_routes: vec![],
        proxy_addr,
        fec_enabled: false,
        fec_k: 4,
        adaptive_fec: Default::default(),
        bypass: Default::default(),
    };
    let mut handle = interceptor.start(config)?;

    // ── (d) Waiting state: nothing installed yet ──────────────────────
    assert!(
        lightspeed_tables().is_empty(),
        "no lightspeed_ table must exist before the first scan"
    );
    assert_eq!(
        handle.snapshot().detected_server,
        None,
        "detected_server must be None while waiting"
    );
    tracing::info!("✅ Waiting state: no rule, detected_server=None");

    // ── (i) No busy-spin during an idle window ────────────────────────
    let cpu_before = process_cpu_ticks();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cpu_after = process_cpu_ticks();
    let idle_ticks = cpu_after.saturating_sub(cpu_before);
    anyhow::ensure!(
        idle_ticks < 50,
        "receive loop appears to be spinning: {idle_ticks} CPU ticks in 1s idle"
    );
    tracing::info!("✅ Idle CPU: {idle_ticks} ticks/1s (no busy-spin)");

    // ── (e) First scan installs the exact-IP rule ─────────────────────
    let needle_a = target_a.ip().to_string();
    anyhow::ensure!(
        wait_for_rule(&needle_a, ROTATE_SCAN_INTERVAL + Duration::from_secs(4)).await,
        "rule for {target_a} was not installed within one scan interval"
    );
    wait_for_detected(&handle, Some(target_a), Duration::from_secs(3)).await?;
    tracing::info!("✅ Rule installed: exact match {target_a}");

    // ── (f) N/N relayed through the mock proxy ────────────────────────
    for _ in 0..N {
        game.send(PAYLOAD).await?;
    }
    anyhow::ensure!(
        wait_for_mock_count(&received, N, Duration::from_secs(3)).await,
        "mock proxy did not receive {N} datagrams"
    );
    let wire = received.lock().unwrap().clone();
    assert!(
        wire.iter().all(|dst| *dst == target_a),
        "every tunneled datagram must carry dst {target_a}: {wire:?}"
    );
    tracing::info!(
        "✅ Mock proxy received {} datagrams, all dst={target_a}",
        wire.len()
    );

    // Replies must reach the *connected* game socket as if from the server.
    let replies = recv_replies(&game, N, Duration::from_secs(3)).await;
    assert_eq!(replies.len(), N, "expected {N} injected replies");
    for (from, data) in &replies {
        assert_eq!(*from, target_a, "injected reply arrived from {from}");
        assert_eq!(data.as_slice(), PAYLOAD, "reply payload mismatch");
    }
    tracing::info!("✅ {N}/{N} replies injected back to the game socket from {target_a}");

    // ── (g) Rotation to .10, no misroute ──────────────────────────────
    //
    // Connect the new socket (which makes the scanner see `.10`, no packets
    // required) but do NOT send until the rule has moved. A pre-swap flow
    // would create a no-NAT conntrack entry that a nat rule added later cannot
    // capture — the mirror image of the fact-3 stickiness for the old flow.
    drop(game);
    let game = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    game.connect(target_b).await?;
    tracing::info!("🔄 Rotated synthetic socket → {target_b} (idle until the rule swaps)");

    let needle_b = target_b.ip().to_string();
    anyhow::ensure!(
        wait_for_rule(&needle_b, ROTATE_SCAN_INTERVAL * 3 + Duration::from_secs(6)).await,
        "rotation to {target_b} did not happen in time"
    );
    wait_for_detected(&handle, Some(target_b), Duration::from_secs(3)).await?;
    tracing::info!("✅ Rotated: rule now matches {target_b}");

    let marker = received.lock().unwrap().len();
    for _ in 0..N {
        game.send(PAYLOAD).await?;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let post_swap = received.lock().unwrap().clone();
    tracing::info!(
        "🔎 post-swap: mock={} marker={} intercepted={} injected={}",
        post_swap.len(),
        marker,
        handle.snapshot().packets_intercepted,
        handle.snapshot().packets_injected
    );
    anyhow::ensure!(
        post_swap.len() > marker,
        "mock proxy received no post-swap datagrams"
    );
    assert!(
        post_swap[marker..].iter().all(|dst| *dst == target_b),
        "post-swap datagrams must all target {target_b}: {:?}",
        &post_swap[marker..]
    );
    assert!(
        !post_swap[marker..].contains(&target_a),
        "no post-swap datagram may still target {target_a}"
    );
    let replies = recv_replies(&game, N, Duration::from_secs(3)).await;
    anyhow::ensure!(!replies.is_empty(), "no replies after rotation");
    assert!(
        replies.iter().all(|(from, _)| *from == target_b),
        "post-swap replies must come from {target_b}: {replies:?}"
    );
    tracing::info!("✅ Post-swap: all traffic dst={target_b}, replies from {target_b}");

    // ── (h) Teardown after the game has no matching route ─────────────
    drop(game);
    let teardown_deadline =
        tokio::time::Instant::now() + ROTATE_SCAN_INTERVAL * 3 + Duration::from_secs(8);
    while tokio::time::Instant::now() < teardown_deadline {
        if lightspeed_tables().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    anyhow::ensure!(
        lightspeed_tables().is_empty(),
        "lightspeed_ table survived with no matching route: {:?}",
        lightspeed_tables()
    );
    anyhow::ensure!(
        handle.snapshot().detected_server.is_none(),
        "detected_server must reset after teardown"
    );
    tracing::info!("✅ Teardown: all lightspeed_ tables removed");

    handle.stop();
    mock_task.abort();
    remove_all_lightspeed_tables();

    tracing::info!("🧹 Cleanup complete");
    tracing::info!("✅ Smoke test PASSED");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers (Linux E2E)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("Uid:"))
                .map(str::to_string)
        })
        .and_then(|line| {
            line.split_whitespace()
                .nth(2)
                .and_then(|euid| euid.parse::<u32>().ok())
        })
        .is_some_and(|euid| euid == 0)
}

#[cfg(target_os = "linux")]
fn command_available(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file()))
}

#[cfg(target_os = "linux")]
fn set_process_name(raw: &[u8]) {
    let mut name = raw.to_vec();
    name.push(0);
    // SAFETY: prctl(PR_SET_NAME) reads at most TASK_COMM_LEN bytes from the
    // NUL-terminated pointer for the duration of the call; `name` outlives it.
    let rc = unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0) };
    if rc != 0 {
        tracing::warn!("prctl(PR_SET_NAME) failed; process matching may not work");
    }
}

#[cfg(target_os = "linux")]
fn lightspeed_tables() -> Vec<String> {
    let Ok(output) = std::process::Command::new("nft")
        .args(["list", "tables"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last().map(str::to_string))
        .filter(|name| name.starts_with("lightspeed_"))
        .collect()
}

#[cfg(target_os = "linux")]
fn rule_text() -> String {
    let mut text = String::new();
    for table in lightspeed_tables() {
        if let Ok(output) = std::process::Command::new("nft")
            .args(["list", "table", "ip", &table])
            .output()
        {
            text.push_str(&String::from_utf8_lossy(&output.stdout));
        }
    }
    text
}

#[cfg(target_os = "linux")]
fn remove_all_lightspeed_tables() {
    for table in lightspeed_tables() {
        let _ = std::process::Command::new("nft")
            .args(["delete", "table", "ip", &table])
            .output();
    }
}

#[cfg(target_os = "linux")]
async fn wait_for_rule(needle: &str, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if rule_text().contains(needle) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    rule_text().contains(needle)
}

#[cfg(target_os = "linux")]
async fn wait_for_detected(
    handle: &crate::interceptor::InterceptorHandle,
    want: Option<SocketAddrV4>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if handle.snapshot().detected_server == want {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    anyhow::bail!(
        "detected_server is {:?}, expected {want:?}",
        handle.snapshot().detected_server
    )
}

#[cfg(target_os = "linux")]
async fn wait_for_mock_count(
    received: &std::sync::Arc<std::sync::Mutex<Vec<SocketAddrV4>>>,
    want: usize,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if received.lock().unwrap().len() >= want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    received.lock().unwrap().len() >= want
}

#[cfg(target_os = "linux")]
async fn recv_replies(
    socket: &tokio::net::UdpSocket,
    want: usize,
    timeout: Duration,
) -> Vec<(SocketAddrV4, Vec<u8>)> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65535];
    let deadline = tokio::time::Instant::now() + timeout;
    while out.len() < want {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, std::net::SocketAddr::V4(from)))) => {
                out.push((from, buf[..len].to_vec()));
            }
            _ => break,
        }
    }
    out
}

/// Process CPU time (`utime + stime`) in USER_HZ ticks from `/proc/self/stat`.
#[cfg(target_os = "linux")]
fn process_cpu_ticks() -> u64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0;
    };
    let Some(rest) = stat.rsplit(')').next() else {
        return 0;
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let utime: u64 = fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);
    utime + stime
}
