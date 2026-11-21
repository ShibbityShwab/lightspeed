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
//!
//! **Portability (2026-11-21).** `tracing` can emit ANSI SGR colour codes (the
//! first Linux CI run did, even to a pipe). The harness therefore strips CSI
//! sequences **before** timestamp masking and spawns the CLI with colour-off
//! hints, so the same four goldens hold on Windows and Linux; a unit test pins
//! the real CI sample and the four simulated-Linux normalizations.

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
///
/// The per-user config base is redirected **inside** `dir` for every invocation
/// so the goldens never depend on the machine's real `%APPDATA%`/`$XDG_CONFIG_HOME`
/// shared config (S6c reads that file for the two settings both front ends own).
/// Fixture tests write their shared config under the same isolated base — see
/// [`write_shared_config`].
///
/// The colour hints below are belt-and-braces: `tracing-subscriber`'s ANSI
/// decision is a build/feature fact, not an environment one (the Linux CI job
/// proved it can emit SGR codes even to a pipe), so the **normalizer stripping
/// CSI sequences** is what makes the goldens portable. The hints are still set
/// so a future logging backend that honours NO_COLOR/CLICOLOR/TERM starts
/// colour-free too — the product's logging config is not touched.
fn run_cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lightspeed"))
        .args(args)
        .current_dir(dir)
        .env("APPDATA", dir.join("iso-appdata"))
        .env("USERPROFILE", dir.join("iso-home"))
        .env("HOME", dir.join("iso-home"))
        .env("XDG_CONFIG_HOME", dir.join("iso-xdg"))
        // No-colour hints for any layer that honours them (the golden harness
        // also strips SGR itself, because tracing's ANSI choice is compile-time).
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        // The zero-spend/telemetry notice must be identical; do not inherit a
        // caller's LIGHTSPEED_* overrides into a golden run.
        .env_remove("LIGHTSPEED_GUI_FORCE")
        .output()
        .expect("run the lightspeed binary")
}

/// The shared per-user config directories the CLI may resolve for the isolated
/// environment [`run_cli`] sets: `<base>/light-speed` on Windows/Linux and
/// `<base>/Library/Application Support/light-speed` on macOS. Fixtures are
/// written to all of them so the matrix is host-independent.
fn shared_config_dirs(dir: &Path) -> Vec<PathBuf> {
    vec![
        dir.join("iso-appdata").join("light-speed"),
        dir.join("iso-xdg").join("light-speed"),
        dir.join("iso-home")
            .join("Library")
            .join("Application Support")
            .join("light-speed"),
    ]
}

/// Write the canonical (`lightspeed.toml`) and/or legacy (`config.toml`) shared
/// config fixtures into the isolated base. `None` leaves that file absent.
fn write_shared_config(dir: &Path, canonical: Option<&str>, legacy: Option<&str>) {
    for shared in shared_config_dirs(dir) {
        std::fs::create_dir_all(&shared).expect("create shared config dir");
        if let Some(body) = canonical {
            std::fs::write(shared.join("lightspeed.toml"), body).expect("write canonical fixture");
        }
        if let Some(body) = legacy {
            std::fs::write(shared.join("config.toml"), body).expect("write legacy fixture");
        }
    }
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

/// Strip ANSI CSI sequences (the SGR colour codes `tracing-subscriber` can emit).
///
/// A CSI sequence is `ESC [` + parameter bytes (0x30–0x3F) + intermediate bytes
/// (0x20–0x2F) + one final byte (0x40–0x7E); SGR (`ESC[…m`) is the shape the
/// Linux CI run emitted around the timestamp, level, target and `:`. An
/// incomplete `ESC [` is kept literally (it is data, not a known sequence), so
/// this never silently deletes text it did not recognise.
fn strip_ansi_csi(input: &str) -> String {
    const ESC: u8 = 0x1b;
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == ESC && index + 1 < bytes.len() && bytes[index + 1] == b'[' {
            let mut end = index + 2;
            while end < bytes.len() && (0x30..=0x3f).contains(&bytes[end]) {
                end += 1;
            }
            while end < bytes.len() && (0x20..=0x2f).contains(&bytes[end]) {
                end += 1;
            }
            if end < bytes.len() && (0x40..=0x7e).contains(&bytes[end]) {
                index = end + 1;
                continue;
            }
            out.push('\u{1b}');
            index += 1;
            continue;
        }
        let ch = input[index..].chars().next().expect("char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// Replace a leading tracing timestamp with `<ts>` and the package version with
/// `v<version>`. Everything else is left byte-for-byte.
///
/// The tracing format is `2026-11-20T14:34:33.760913Z  INFO target: message`;
/// only the leading `...Z` is volatile. ANSI CSI sequences are stripped **before**
/// masking, because a coloured trace line starts with `ESC[2m` and the timestamp
/// matcher would no longer see the timestamp at the start of the line (that is
/// exactly how the first Linux CI run failed all four goldens).
fn normalize_common(raw: &str) -> String {
    let version = format!("v{VERSION}");
    let mut out = Vec::new();
    for line in raw.lines() {
        let plain = strip_ansi_csi(line);
        let masked = mask_timestamp(&plain).replace(&version, "v<version>");
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
/// result lines (none-detected, auto-detected, targeting) and mask the live
/// probe latency. The rest of the report (candidate count, configured candidate,
/// status word) is exact. Host process state must not leak into the golden: a
/// machine with a matching game running and one without produce the same bytes.
fn normalize_probe(raw: &str) -> String {
    let normalized = normalize_common(raw);
    let mut out = Vec::new();
    for line in normalized.lines() {
        if line.contains("No supported game detected")
            || line.trim_start().starts_with("Supported: ")
            || line.contains("Detected game")
            || line.contains("Auto-detected game")
            || line.contains("Targeting game:")
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

// ── Shared per-user config fixtures (S6c / Q10) ────────────────────────────
//
// The frozen goldens above pass `--config absent.toml`, i.e. the CLI has no own
// config file, so the shared per-user `AppConfig` supplies the two settings both
// front ends own. These fixtures drive that path with an isolated config base
// (see `run_cli`). The observable, deterministic CLI effect is the telemetry
// notice: `share_latency_stats` is the shared consent bit, so the notice's
// presence/absence proves which file won. No network, no new golden files.

const TELEMETRY_NOTICE_FRAGMENT: &str = "Anonymous telemetry on";

fn telemetry_notice(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stdout).contains(TELEMETRY_NOTICE_FRAGMENT)
}

#[test]
fn shared_config_legacy_only_is_imported_by_the_cli() {
    let dir = scratch_dir("shared-legacy-only");
    // Old-only: the pre-unification GUI file carries consent=false and the
    // retired key; the read-only import must apply it and tolerate the key.
    write_shared_config(
        &dir,
        None,
        Some("retry_after_fallback = true\nshare_latency_stats = false\n"),
    );
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        !telemetry_notice(&out),
        "the legacy GUI file's consent=false must reach the CLI through the read-only import"
    );
}

#[test]
fn shared_config_canonical_wins_over_the_legacy_file() {
    let dir = scratch_dir("shared-both");
    // Both: canonical consent=true (the shipped default) wins over legacy false.
    write_shared_config(
        &dir,
        Some("share_latency_stats = true\n"),
        Some("share_latency_stats = false\n"),
    );
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        telemetry_notice(&out),
        "the canonical value must win over the legacy GUI file (Q10)"
    );
}

#[test]
fn shared_config_new_only_sets_the_cli_defaults() {
    let dir = scratch_dir("shared-new-only");
    write_shared_config(&dir, Some("share_latency_stats = false\n"), None);
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        !telemetry_notice(&out),
        "canonical-only consent=false applies"
    );
}

#[test]
fn shared_config_malformed_canonical_keeps_the_cli_defaults() {
    let dir = scratch_dir("shared-malformed");
    // A broken canonical file is a GUI recovery; the CLI must keep its defaults
    // and must not mix in the legacy values.
    write_shared_config(
        &dir,
        Some("this is not = = valid {{ toml\n"),
        Some("share_latency_stats = false\n"),
    );
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        telemetry_notice(&out),
        "a malformed canonical file falls back to defaults, not to legacy values"
    );
}

/// The CLI's own `--config` file still wins over the shared per-user config: the
/// shared file is only the fallback when `--config` is unreadable.
#[test]
fn own_config_file_wins_over_the_shared_config() {
    let dir = scratch_dir("own-config-wins");
    write_shared_config(&dir, Some("share_latency_stats = false\n"), None);
    std::fs::write(dir.join("own.toml"), "[general]\ntelemetry = true\n")
        .expect("write own config");
    let out = run_cli(
        &dir,
        &["--list-games", "--no-user-profiles", "--config", "own.toml"],
    );
    assert!(out.status.success());
    assert!(
        telemetry_notice(&out),
        "the CLI's own config must win over the shared per-user config"
    );
}

/// A GUI "Reset to defaults" (the explicit-defaults marker) reaches the CLI: the
/// legacy file must be skipped even though it carries old consent=false, so the
/// CLI sees default consent (notice on).
#[test]
fn cli_sees_the_gui_reset_as_defaults() {
    let dir = scratch_dir("shared-reset");
    write_shared_config(
        &dir,
        Some("[meta]\nlegacy_import = false\n"),
        Some("share_latency_stats = false\nselected = \"1.1.1.1:4434\"\n"),
    );
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        telemetry_notice(&out),
        "the reset marker must suppress the legacy import in the CLI too"
    );
}

/// After a reset, a canonical value the user then sets still wins: the marker
/// suppresses only the legacy import, not the canonical file.
#[test]
fn cli_honours_canonical_values_after_a_reset() {
    let dir = scratch_dir("shared-reset-canonical");
    // NOTE the TOML scoping: the `[meta]` header starts a table, so the value
    // the user set must come *before* it (top level).
    write_shared_config(
        &dir,
        Some("share_latency_stats = false\n[meta]\nlegacy_import = false\n"),
        Some("share_latency_stats = true\n"),
    );
    let out = run_cli(
        &dir,
        &[
            "--list-games",
            "--no-user-profiles",
            "--config",
            "absent.toml",
        ],
    );
    assert!(out.status.success());
    assert!(
        !telemetry_notice(&out),
        "the canonical consent=false must apply after a reset"
    );
}

// ── ANSI portability (Linux CI) ─────────────────────────────────────────────
//
// First PR CI run (2026-11-21): the four goldens failed only on Linux because
// `tracing-subscriber` wrote SGR colour codes around the timestamp/level/target
// on that job. Sample reproduced from
// `wat/evidence/2026-11-21/ci-run/failed-check-test.log:1867`.

/// The exact raw startup line the Linux CI job produced (ANSI SGR + RFC3339 Z).
fn ci_ansi_startup_line() -> String {
    format!(
        "\u{1b}[2m2026-11-21T03:52:55.652263Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \
         \u{1b}[2mlightspeed\u{1b}[0m\u{1b}[2m:\u{1b}[0m ⚡ LightSpeed v{VERSION} starting"
    )
}

#[test]
fn ansi_sequences_are_stripped_before_timestamp_masking() {
    let normalized = normalize_common(&ci_ansi_startup_line());
    assert_eq!(
        normalized, "<ts>  INFO lightspeed: ⚡ LightSpeed v<version> starting",
        "the Linux CI raw line must normalize to the frozen golden line"
    );
    // Stripping is not start-of-line only, and it is idempotent on plain text.
    assert_eq!(
        strip_ansi_csi("plain \u{1b}[1mbold\u{1b}[0m text"),
        "plain bold text"
    );
    let plain = "<ts>  INFO lightspeed: ⚡ LightSpeed v<version> starting";
    assert_eq!(strip_ansi_csi(plain), plain);
    // An incomplete escape is data, not a sequence: keep it.
    assert_eq!(strip_ansi_csi("a\u{1b}[12"), "a\u{1b}[12");
}

#[test]
fn timestamp_masking_handles_rfc3339_and_platform_shapes() {
    // RFC3339 `Z` (the real form on both platforms): masked to `<ts>`.
    assert_eq!(
        mask_timestamp("2026-11-21T03:52:55.652263Z  INFO lightspeed: x"),
        "<ts>  INFO lightspeed: x"
    );
    // Windows raw output: the same RFC3339 `Z` line with a CRLF ending; `lines()`
    // drops the CR before masking, so the golden form is produced.
    let windows = format!(
        "2026-11-20T14:34:33.760913Z  INFO lightspeed: ⚡ LightSpeed v{VERSION} starting\r\n"
    );
    assert_eq!(
        normalize_common(&windows),
        "<ts>  INFO lightspeed: ⚡ LightSpeed v<version> starting"
    );
    // A line without a leading timestamp is untouched (structural rows).
    assert_eq!(
        mask_timestamp("   Configured relays: 12"),
        "   Configured relays: 12"
    );
}

#[test]
fn simulated_linux_raw_output_normalizes_to_the_frozen_goldens() {
    type Normalizer = fn(&str) -> String;
    // Each frozen golden, with its startup line replaced by the raw Linux CI
    // shape, must normalize back to the committed bytes: the four goldens stay
    // hash-identical without regeneration.
    let cases: [(&str, Normalizer); 4] = [
        ("list-games.normalized.txt", normalize_common),
        ("malformed-config.normalized.txt", normalize_common),
        ("status.normalized.txt", normalize_status),
        ("probe-proxies.normalized.txt", normalize_probe),
    ];
    for (name, normalize) in cases {
        let golden = std::fs::read_to_string(golden_path(name)).expect("read golden");
        let golden = golden.strip_suffix('\n').unwrap_or(&golden).to_string();
        let mut lines: Vec<String> = golden.lines().map(str::to_string).collect();
        assert!(
            lines[0].starts_with("<ts>  INFO lightspeed: "),
            "{name}: the first line is the masked startup line"
        );
        lines[0] = ci_ansi_startup_line();
        let raw = lines.join("\n");
        assert_eq!(
            normalize(&raw),
            golden,
            "simulated Linux raw output must normalize to frozen golden {name}"
        );
    }
}
