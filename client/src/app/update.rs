//! Pure update rules shared by every front end — S6 of the GUI/core migration.
//!
//! WF-081 W2-3b S6 (`docs/design/gui-core-boundary.md` §5.6): the version
//! comparison, the release-asset coordinates and the checksum-file parser are
//! pure rules, so they live in the shared core instead of the Windows GUI.
//! Everything that talks to the network, verifies a download or raises
//! elevation stays in `client-gui/src/{update,self_update}.rs`; the core adds
//! **no dependencies** (this module is std-only).
//!
//! The update check is user-initiated only (A11): nothing here runs on startup.

/// Download coordinates for the in-place (receipt-less) installer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// The release tag, e.g. `v1.6.14`.
    pub tag: String,
    /// Browser download URL of the GUI zip.
    pub zip_url: String,
    /// Browser download URL of the zip's `.sha256` file.
    pub sha256_url: String,
}

/// Returns whether `latest` is newer than `current`.
///
/// Semver-aware and tolerant of a leading `v`/`V` prefix. Numeric segments are
/// compared as unsigned integers; non-numeric or empty segments are treated as
/// `0`, so malformed input never panics and never compares greater than a
/// well-formed version sharing the same numeric prefix.
pub fn compare_versions(current: &str, latest: &str) -> bool {
    let current_segments = numeric_segments(strip_v_prefix(current));
    let latest_segments = numeric_segments(strip_v_prefix(latest));

    let count = current_segments.len().max(latest_segments.len());
    for index in 0..count {
        let current_segment = current_segments.get(index).copied().unwrap_or(0);
        let latest_segment = latest_segments.get(index).copied().unwrap_or(0);
        match current_segment.cmp(&latest_segment) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

fn strip_v_prefix(version: &str) -> &str {
    let version = version.trim();
    if let Some(stripped) = version.strip_prefix('v') {
        stripped
    } else if let Some(stripped) = version.strip_prefix('V') {
        stripped
    } else {
        version
    }
}

fn numeric_segments(version: &str) -> Vec<u64> {
    version.split('.').map(leading_number).collect()
}

fn leading_number(segment: &str) -> u64 {
    let mut value: u64 = 0;
    for ch in segment.chars() {
        match ch.to_digit(10) {
            Some(digit) => value = value.saturating_mul(10).saturating_add(u64::from(digit)),
            None => break,
        }
    }
    value
}

/// First hex token of a `sha256sum`-style file (`<hex>  <filename>`).
/// Tolerant of a trailing CR and of extra fields.
pub fn parse_sha256_file(text: &str) -> Result<String, String> {
    let token = text
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty checksum file".to_string())?;
    if token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(token.to_string())
    } else {
        Err(format!("malformed checksum file: first token is {token:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compare_versions ───────────────────────────────────────────────────

    #[test]
    fn latest_patch_bump_is_newer() {
        assert!(compare_versions("1.3.2", "v1.3.3"));
    }

    #[test]
    fn equal_versions_are_not_newer() {
        assert!(!compare_versions("1.3.2", "1.3.2"));
    }

    #[test]
    fn older_latest_is_not_newer() {
        assert!(!compare_versions("1.4.0", "1.3.9"));
    }

    #[test]
    fn multi_digit_segment_compared_numerically() {
        assert!(compare_versions("1.3.9", "1.3.10"));
    }

    #[test]
    fn leading_v_prefix_is_tolerated() {
        assert!(compare_versions("v1.3.2", "v1.3.3"));
        assert!(compare_versions("1.3.2", "V1.3.3"));
        assert!(!compare_versions("1.3.3", "V1.3.3"));
    }

    #[test]
    fn missing_segments_compare_as_zero() {
        assert!(compare_versions("1.3", "1.3.1"));
        assert!(!compare_versions("1.3.1", "1.3"));
    }

    #[test]
    fn malformed_and_empty_segments_do_not_panic() {
        assert!(!compare_versions("", ""));
        assert!(compare_versions("", "1.0.0"));
        assert!(!compare_versions("abc", "abc"));
        assert!(compare_versions("abc", "1.0.0"));
        assert!(!compare_versions("1.3.2", "1.3.2-beta"));
        assert!(!compare_versions("1..2", "1..2"));
    }

    // ── parse_sha256_file ──────────────────────────────────────────────────

    #[test]
    fn sha256_file_parses_the_hex_token() {
        let hex = "a".repeat(64);
        let parsed = parse_sha256_file(&format!("{hex}  lightspeed-gui.zip\r\n")).unwrap();
        assert_eq!(parsed, hex);

        // Uppercase hex is accepted; the comparison is case-insensitive.
        assert!(parse_sha256_file(&format!("{}  x.zip", hex.to_uppercase())).is_ok());
    }

    #[test]
    fn sha256_file_rejects_garbage() {
        assert!(parse_sha256_file("").is_err(), "empty input");
        assert!(parse_sha256_file("short").is_err(), "short token");
        assert!(
            parse_sha256_file(&format!("{}  x.zip", "z".repeat(64))).is_err(),
            "non-hex token"
        );
    }
}
