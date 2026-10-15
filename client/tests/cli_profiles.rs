//! End-to-end asserts for the `--profiles` CLI surface.
//!
//! Both cases spawn the built `lightspeed` binary rather than the library,
//! because each is exactly what a user sees: the store is initialized from
//! `--profiles`, then `games::resolve()` serves the list and the error.
//!
//! 1. A user game is listed under `Your profiles` (`--list-games`, exit 0).
//! 2. An unknown key still reports the built-in `Unknown game` head, with the
//!    loaded user keys appended as a `Your profiles:` line.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A temp dir holding one `profiles.toml`, unique per process and call.
fn profiles_file(body: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lightspeed-cli-profiles-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("profiles.toml");
    std::fs::write(&path, body).expect("write profiles.toml");
    path
}

/// One valid user game: a process name plus a UDP range (the schema requires
/// at least one of the two), under a key no built-in claims.
const PROFILE: &str = r#"
version = 1

[[games]]
name = "My Test Game"
key = "my-test-game"
process_names = ["mytestgame.exe"]
udp_ports = "27015-27050"
"#;

/// Run the shipped binary and return (exit code, stdout+stderr).
fn run(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_lightspeed"))
        .args(args)
        .output()
        .expect("spawn the lightspeed binary");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn lists_a_user_profile_under_your_profiles() {
    let path = profiles_file(PROFILE);
    let (ok, text) = run(&["--list-games", "--profiles", path.to_str().unwrap()]);
    assert!(ok, "--list-games must exit 0:\n{text}");

    let builtin_at = text
        .find("Built-in games")
        .unwrap_or_else(|| panic!("no built-in section:\n{text}"));
    let user_at = text
        .find("Your profiles")
        .unwrap_or_else(|| panic!("no user section:\n{text}"));
    assert!(builtin_at < user_at, "built-ins must come first:\n{text}");
    assert!(text.contains("my-test-game"), "{text}");
    assert!(text.contains("My Test Game"), "{text}");
}

#[test]
fn an_unknown_key_reports_the_extended_error_shape() {
    let path = profiles_file(PROFILE);
    let (ok, text) = run(&[
        "--scan-processes",
        "--game",
        "no-such-test-game",
        "--profiles",
        path.to_str().unwrap(),
    ]);
    assert!(!ok, "an unknown key must fail:\n{text}");
    assert!(text.contains("Unknown game: 'no-such-test-game'"), "{text}");
    assert!(text.contains("Supported:"), "{text}");
    assert!(
        text.contains("Your profiles: my-test-game"),
        "the loaded user key must be listed:\n{text}"
    );
}
