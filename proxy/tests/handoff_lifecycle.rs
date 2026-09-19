//! Integration tests for the handoff CLI surface.
//!
//! `--handoff-schema` must print the manifest schema version, and
//! `--handoff-validate` must accept a well-formed manifest (real UDP fds with
//! the correct self sha256) and reject one whose outbound fd is a regular file
//! or whose target sha256 is wrong. Both modes are pure: they never bind or
//! serve.

#![cfg(target_os = "linux")]

use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::process::{Command, Output};

use lightspeed_proxy::handoff::{
    clear_cloexec, sha256_file, write_json_atomic, AuthTokenSnapshot, HandoffManifest,
    SessionSnapshot, HANDOFF_SCHEMA_VERSION,
};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "ls-handoff-lifecycle-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn proxy_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lightspeed-proxy"))
}

fn proxy_self_sha() -> String {
    sha256_file(&proxy_bin()).unwrap()
}

fn run(args: &[&str]) -> Output {
    Command::new(proxy_bin())
        .args(args)
        .output()
        .expect("spawn lightspeed-proxy")
}

/// Build a manifest whose fds are the caller's real sockets.
fn manifest_with_fds(data_fd: RawFd, outbound_fd: RawFd, to_sha256: String) -> HandoffManifest {
    HandoffManifest {
        schema_version: HANDOFF_SCHEMA_VERSION,
        handoff_id: "test-handoff".to_string(),
        from_version: "0.0.0".to_string(),
        to_version: env!("CARGO_PKG_VERSION").to_string(),
        to_sha256,
        created_at_unix_ms: 1_700_000_000_000,
        data_fd,
        tcp_fd: None,
        proxy_started_at_unix_ms: 1_699_999_000_000,
        sessions: vec![SessionSnapshot {
            client_addr: "127.0.0.1:40000".to_string(),
            game_server: "127.0.0.1:27015".to_string(),
            outbound_fd,
            fec_enabled: false,
            fec_k: 4,
            age_us: 1000,
            idle_us: 100,
            response_seq: 0,
            last_client_seq: 0,
            packets_relayed: 0,
            bytes_relayed: 0,
        }],
        auth: vec![AuthTokenSnapshot {
            token: 7,
            principal: "127.0.0.1".to_string(),
            bound_port: 40000,
            ttl_ms_remaining: 60_000,
        }],
    }
}

#[test]
fn handoff_schema_prints_the_manifest_schema_version() {
    let output = run(&["--handoff-schema"]);
    assert!(
        output.status.success(),
        "status: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        HANDOFF_SCHEMA_VERSION.to_string()
    );
}

#[test]
fn handoff_validate_accepts_real_sockets() {
    let dir = temp_dir("valid");
    let manifest_path = dir.join("handoff.json");
    let data = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let outbound = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    clear_cloexec(data.as_raw_fd()).unwrap();
    clear_cloexec(outbound.as_raw_fd()).unwrap();
    let manifest = manifest_with_fds(data.as_raw_fd(), outbound.as_raw_fd(), proxy_self_sha());
    write_json_atomic(&manifest_path, &manifest).unwrap();

    let output = run(&["--handoff-validate", manifest_path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "valid manifest rejected\nstatus: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    drop((data, outbound));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn handoff_validate_rejects_a_regular_file_outbound_fd() {
    let dir = temp_dir("regular");
    let manifest_path = dir.join("handoff.json");
    let data = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    clear_cloexec(data.as_raw_fd()).unwrap();

    let file_path = dir.join("not-a-socket");
    std::fs::write(&file_path, b"nope").unwrap();
    let file = std::fs::File::open(&file_path).unwrap();
    clear_cloexec(file.as_raw_fd()).unwrap();

    let manifest = manifest_with_fds(data.as_raw_fd(), file.as_raw_fd(), proxy_self_sha());
    write_json_atomic(&manifest_path, &manifest).unwrap();

    let output = run(&["--handoff-validate", manifest_path.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "regular-file fd must be rejected\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    drop(file);
    drop(data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn handoff_validate_rejects_a_wrong_sha() {
    let dir = temp_dir("sha");
    let manifest_path = dir.join("handoff.json");
    let data = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let outbound = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    clear_cloexec(data.as_raw_fd()).unwrap();
    clear_cloexec(outbound.as_raw_fd()).unwrap();
    let manifest = manifest_with_fds(data.as_raw_fd(), outbound.as_raw_fd(), "0".repeat(64));
    write_json_atomic(&manifest_path, &manifest).unwrap();

    let output = run(&["--handoff-validate", manifest_path.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "wrong sha must be rejected\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    drop((data, outbound));
    let _ = std::fs::remove_dir_all(&dir);
}
