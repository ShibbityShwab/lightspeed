//! # Self-Update State (`/health`)
//!
//! The updater writes a small JSON document describing which release is running
//! and how the last self-update attempt went. The health endpoint surfaces it so
//! an operator can see that from `/health` without shelling into the host.
//!
//! The file is untrusted input: it may be missing, unreadable, truncated, or
//! malformed, and it may be replaced at any moment. Nothing in this module may
//! fail, panic, or block the health endpoint, so every error collapses to
//! `None`, the read is hard-capped, and the parsed snapshot is cached briefly
//! so frequent health polling does not hit the filesystem on every request.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// State file path when `LIGHTSPEED_UPDATE_STATE` is unset.
const DEFAULT_STATE_PATH: &str = "/var/lib/lightspeed/update-state.json";

/// Environment variable that overrides the state file path.
const STATE_PATH_ENV: &str = "LIGHTSPEED_UPDATE_STATE";

/// Hard cap on bytes read from the state file.
const MAX_STATE_BYTES: usize = 64 * 1024;

/// How long a parsed snapshot is served before the file is re-read.
const CACHE_TTL: Duration = Duration::from_secs(1);

/// Longest string echoed per field, so a hostile file cannot bloat `/health`.
const MAX_STRING_CHARS: usize = 512;

/// Operator-facing snapshot of the last self-update attempt.
///
/// `schema_version` is intentionally not modelled: `serde` ignores unknown
/// fields, and the updater's schema is the source of truth for the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    pub current_version: String,
    pub active_release: String,
    #[serde(default)]
    pub available_version: Option<String>,
    #[serde(default)]
    pub rollback_version: Option<String>,
    pub last_check_at: u64,
    #[serde(default)]
    pub last_apply_at: Option<u64>,
    pub last_result: String,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl UpdateState {
    /// Truncate every string field to a bounded length, on a char boundary.
    fn sanitize(&mut self) {
        cap_string(&mut self.current_version);
        cap_string(&mut self.active_release);
        cap_string(&mut self.last_result);
        if let Some(v) = self.available_version.as_mut() {
            cap_string(v);
        }
        if let Some(v) = self.rollback_version.as_mut() {
            cap_string(v);
        }
        if let Some(v) = self.last_error.as_mut() {
            cap_string(v);
        }
    }
}

/// Truncate `s` to [`MAX_STRING_CHARS`] characters without splitting a char.
fn cap_string(s: &mut String) {
    if s.chars().count() <= MAX_STRING_CHARS {
        return;
    }
    let end = s
        .char_indices()
        .nth(MAX_STRING_CHARS)
        .map_or(s.len(), |(i, _)| i);
    s.truncate(end);
}

/// Cached snapshot plus the path and time it was read from.
struct CacheEntry {
    path: PathBuf,
    loaded_at: Instant,
    state: Option<UpdateState>,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

/// Resolve the state file path, re-reading the env var on every call so tests
/// and operators can redirect it without a restart.
fn state_path() -> PathBuf {
    match std::env::var_os(STATE_PATH_ENV) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_STATE_PATH),
    }
}

/// The current self-update state, or `None` when it is unavailable.
///
/// Never fails: a missing, unreadable, oversized, or malformed file yields
/// `None`. The parsed result is cached for [`CACHE_TTL`], and a change to the
/// resolved path forces an immediate refresh so a redirected path never serves
/// a snapshot from the old one.
pub fn current_update_state() -> Option<UpdateState> {
    let path = state_path();
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(entry) = guard.as_ref() {
        if entry.path == path && entry.loaded_at.elapsed() < CACHE_TTL {
            return entry.state.clone();
        }
    }
    let state = read_state_file(&path);
    *guard = Some(CacheEntry {
        path,
        loaded_at: Instant::now(),
        state: state.clone(),
    });
    state
}

/// Read and parse the state file. Any failure at all yields `None`.
fn read_state_file(path: &Path) -> Option<UpdateState> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    // Read one byte past the cap so an oversized file is detected, not truncated
    // into something that happens to parse.
    let mut limited = file.take((MAX_STATE_BYTES + 1) as u64);
    limited.read_to_end(&mut buf).ok()?;
    if buf.len() > MAX_STATE_BYTES {
        return None;
    }
    let mut state: UpdateState = serde_json::from_slice(&buf).ok()?;
    state.sanitize();
    Some(state)
}

/// Drop the cache. Tests that repoint the env var call this for determinism.
#[cfg(test)]
pub(crate) fn reset_cache_for_test() {
    *CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}
