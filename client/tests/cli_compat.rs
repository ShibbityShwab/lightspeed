//! CLI compatibility goldens for WF-081 W2-3b **S6**.
//!
//! The CLI adoption slice (`docs/design/gui-core-boundary.md` §6 S6) must not
//! change flags, exit codes or user-visible text. These tests freeze the
//! user-visible output of the four paths that touch shared-core state before the
//! adoption starts, so the adoption is judged against the shipped behaviour:
//!
//! * `--list-games` — exact output (the built-in game catalogue);
//! * a malformed `--config` — exact output (tolerant loader: warn + defaults);
//! * `--status` — normalized output (live values masked: timestamps, version,
//!   host OS/arch/features; the environment-dependent tail is asserted
//!   structurally in order);
//! * `--probe-proxies` — normalized output (the auto-detect result and the live
//!   probe latency are masked; the report shape and the configured candidate are
//!   exact).
//!
//! The goldens were generated from the accepted S5b tree **before** any S6 code
//! path changed and live in `client/tests/cli_goldens/`. They only mask values
//! that differ between runs/hosts: wall-clock timestamps, the package version,
//! host OS/arch/compile features, the game auto-detect result (host processes)
//! and live latencies. Everything else is compared byte-for-byte.
//!
//! Regenerating (only when a deliberate CLI text change is reviewed):
//! `LIGHTSPEED_CLI_GOLDEN_UPDATE=1 cargo test -p lightspeed-client --test cli_compat`
//! — then READ the golden diff; never regenerate to make a failing test pass.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The package version the CLI prints; masked as `v<version>` in the goldens.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("client/ has a workspace parent")
        .to_path_buf()
}

/// A clean scratch directory for one test, under the workspace `target/`
/// (writable by the low-integrity test binary on this host — see
/// `wat/evidence` note and the `lightspeed-windows` skill).
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = workspace_root().join("target").join("cli-compat").join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the shipped CLI binary in `dir` and return its captured output.
fn run_cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lightspeed"))
        .args(args)
        .current_dir(dir)
        // The zero-spend/telemetry notice must be identical; do not inherit a
        // caller's LIGHTSPEED_* overrides into a golden run.
        .env_remove("LIGHTSPEED_GUI_FORCE")
        .output()
        .expect("run the lightspeed binary")
}

fn stdout_of(out: &Output) -> String {
    assert!(
        out.status.success(),
        "the CLI must exit 0 for this command (got {:?}); stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).expect("CLI stdout is UTF-8")
}

/// Replace a leading tracing timestamp with `<ts>` and the package version with
/// `v<version>`. Everything else is left byte-for-byte.
///
/// The tracing format is `2026-11-20T14:34:33.760913Z  INFO target: message`;
/// only the leading `...Z` is volatile.
fn normalize_common(raw: &str) -> String {
    let version = format!("v{VERSION}");
    let mut out = Vec::new();
    for line in raw.lines() {
        let masked = mask_timestamp(line).replace(&version, "v<version>");
        out.push(masked);
    }
    out.join("\n")
}

fn mask_timestamp(line: &str) -> String {
    if let Some(z) = line.find('Z') {
        let head = &line[..z];
        let looks_like_rfc3339 =
            head.len() >= 20 && head.as_bytes()[4] == b'-' && head.contains('T');
        if looks_like_rfc3339 {
            return format!("<ts>{}", &line[z + 1..]);
        }
    }
    line.to_string()
}

/// `--status` normalization: mask the host facts, then keep only the stable
/// prefix up to the `Features:` row. The environment-dependent tail (interceptor
/// probe, privilege check, rules, running games) is asserted structurally.
fn normalize_status(raw: &str) -> String {
    let normalized = normalize_common(raw);
    let mut out = Vec::new();
    for line in normalized.lines() {
        let masked = if line.contains("   OS:         ") {
            line.replace(
                line.split("   OS:         ").nth(1).unwrap_or_default(),
                "<os>",
            )
        } else if line.contains("   Arch:       ") {
            line.replace(
                line.split("   Arch:       ").nth(1).unwrap_or_default(),
                "<arch>",
            )
        } else if line.contains("   Features:   ") {
            line.replace(
                line.split("   Features:   ").nth(1).unwrap_or_default(),
                "<features>",
            )
        } else {
            line.to_string()
        };
        let is_features_row = masked.contains("   Features:   <features>");
        out.push(masked);
        if is_features_row {
            break;
        }
    }
    out.join("\n")
}

/// `--probe-proxies` normalization: drop the host-dependent game auto-detect
/// result lines and mask the live probe latency. The rest of the report
/// (candidate count, configured candidate, status word) is exact.
fn normalize_probe(raw: &str) -> String {
    let normalized = normalize_common(raw);
    let mut out = Vec::new();
    for line in normalized.lines() {
        if line.contains("No supported game detected")
            || line.trim_start().starts_with("Supported: ")
            || line.contains("Detected game")
        {
            continue;
        }
        let masked = mask_trailing_latency(line);
        out.push(masked);
    }
    out.join("\n")
}

/// Replace a trailing ` - timeout` / ` - 12.3ms` with ` - <latency>`.
fn mask_trailing_latency(line: &str) -> String {
    let Some(split) = line.rfind(" - ") else {
        return line.to_string();
    };
    let suffix = &line[split + 3..];
    let is_latency = suffix == "timeout"
        || (suffix.ends_with("ms")
            && suffix
                .trim_end_matches("ms")
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.'));
    if is_latency {
        format!("{} - <latency>", &line[..split])
    } else {
        line.to_string()
    }
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cli_goldens")
        .join(name)
}

/// Compare `actual` with the committed golden, or write it when the deliberate
/// regeneration mode is on. A mismatch reports the first differing line.
fn assert_golden(name: &str, actual: &str) {
    if std::env::var("LIGHTSPEED_CLI_GOLDEN_UPDATE").as_deref() == Ok("1") {
        std::fs::write(golden_path(name), format!("{actual}\n")).expect("write golden");
        eprintln!("wrote golden {name} ({} lines)", actual.lines().count());
        return;
    }
    let expected = std::fs::read_to_string(golden_path(name))
        .unwrap_or_else(|e| panic!("missing golden {name}: {e}"));
    let expected = expected.strip_suffix('\n').unwrap_or(&expected);
    if actual == expected {
        return;
    }
    let mut diff = String::new();
    for (index, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
        if want != got {
            diff.push_str(&format!(
                "line {}:\n  expected: {want}\n  actual:   {got}\n",
                index + 1
            ));
            break;
        }
    }
    panic!(
        "CLI golden `{name}` changed ({} vs {} lines).\n{diff}\
         The CLI adoption must not change user-visible output; if the change is \
         deliberate, regenerate with LIGHTSPEED_CLI_GOLDEN_UPDATE=1 and review the diff.",
        expected.lines().count(),
        actual.lines().count()
    );
}

/// `--list-games` must print the exact built-in catalogue.
#[test]
fn list_games_output_is_exact() {
    let dir = scratch_dir("list-games");
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.stderr.is_empty(), "the CLI writes its report to stdout");
    assert_golden(
        "list-games.normalized.txt",
        &normalize_common(&stdout_of(&out)),
    );
}

/// A malformed `--config` is tolerated: the loader warns with the exact parse
/// error and continues on defaults (exit 0, same output shape).
#[test]
fn malformed_config_output_is_exact() {
    let dir = scratch_dir("malformed-config");
    std::fs::write(
        dir.join("malformed.toml"),
        "this is not = = valid {{ toml\n",
    )
    .expect("write malformed config");
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "malformed.toml",
        ],
    );
    assert_golden(
        "malformed-config.normalized.txt",
        &normalize_common(&stdout_of(&out)),
    );
}

/// `--status` prints the stable system header; the host-dependent sections are
/// asserted to still exist and keep their order.
#[test]
fn status_output_is_normalized_and_keeps_its_sections() {
    let dir = scratch_dir("status");
    let out = run_cli(
        &dir,
        &["--status", "--no-user-profiles", "--config", "absent.toml"],
    );
    let normalized = normalize_status(&stdout_of(&out));
    assert_golden("status.normalized.txt", &normalized);

    let full = normalize_common(&stdout_of(&out));
    let interceptor = full
        .find("   Interceptor: ")
        .expect("--status reports the interceptor backend");
    let games = full
        .find("   Games: ")
        .expect("--status reports the running-game scan");
    assert!(
        interceptor < games,
        "--status section order changed: interceptor must precede games"
    );
}

/// `--probe-proxies` prints the exact report shape for the configured candidate;
/// the auto-detect result and the measured latency are masked as live values.
#[test]
fn probe_proxies_output_is_normalized() {
    let dir = scratch_dir("probe-proxies");
    // A closed loopback port: the probe times out deterministically without
    // touching the network, and the registry URL is rejected as non-HTTPS
    // before any request, so this golden needs no network.
    std::fs::write(
        dir.join("probe.toml"),
        "[proxy]\nservers = [\"127.0.0.1:9\"]\n\n[registry]\nurl = \"http://127.0.0.1:9/nodes.json\"\n",
    )
    .expect("write probe config");
    let out = run_cli(
        &dir,
        &[
            "--probe-proxies",
            "--no-user-profiles",
            "--config",
            "probe.toml",
        ],
    );
    assert_golden(
        "probe-proxies.normalized.txt",
        &normalize_probe(&stdout_of(&out)),
    );
    assert!(
        out.stderr.is_empty(),
        "the plain-text proxy report goes to stdout"
    );
}
