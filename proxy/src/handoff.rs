//! # In-place handoff manifest and fd transfer helpers
//!
//! Phase 2a of the single-process `execve` handoff: the data structures, the
//! JSON manifest/request on-disk format, and the fd/state transfer primitives.
//!
//! A new binary can adopt the data listener and every per-session outbound UDP
//! fd plus the auth table, so existing UDP client flows continue without a
//! reconnect.  FEC decoder state and global metrics are intentionally reset,
//! and TCP sessions are intentionally dropped: only the state a client can
//! observe as continuous is carried across.
//!
//! Nothing in this module wires signals, `execve`, or startup adoption (that is
//! Phase 2b).  It is the pure, testable core: serialize, read with a hard size
//! cap and schema check, validate untrusted fds, and verify a handoff request's
//! binary before it is ever executed.

use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// On-disk manifest/request schema version understood by this build.
pub const HANDOFF_SCHEMA_VERSION: u32 = 1;

/// Default manifest path written by the outgoing process.
pub const DEFAULT_MANIFEST_PATH: &str = "/run/lightspeed/handoff.json";

/// Default handoff-request path polled by the outgoing process.
pub const DEFAULT_REQUEST_PATH: &str = "/run/lightspeed/handoff-request.json";

/// Default root containing the versioned release directories.
pub const DEFAULT_RELEASES_DIR: &str = "/opt/lightspeed/releases";

/// Default path of the last handoff result, surfaced through `/health`.
pub const DEFAULT_RESULT_PATH: &str = "/run/lightspeed/handoff-result.json";

/// Environment variable that carries the manifest path into the new process.
///
/// It is the adoption trigger: a process that finds it set adopts the manifest
/// instead of binding the data socket. It also overrides the manifest write
/// path so a test can drive the sequence without touching `/run`.
pub const MANIFEST_ENV: &str = "LIGHTSPEED_HANDOFF_MANIFEST";

/// Environment variable that overrides the release root.
pub const RELEASES_DIR_ENV: &str = "LIGHTSPEED_RELEASES_DIR";

/// Environment variable that overrides the result path (tests only).
pub const RESULT_PATH_ENV: &str = "LIGHTSPEED_HANDOFF_RESULT";

/// Whether in-place handoff is supported for this target.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// [`HandoffStatus::result`] value for a completed handoff.
pub const RESULT_OK: &str = "ok";
/// [`HandoffStatus::result`] value for a request that was refused.
pub const RESULT_REJECTED: &str = "rejected";
/// [`HandoffStatus::result`] value for a handoff that failed after acceptance.
pub const RESULT_FAILED: &str = "failed";

/// Hard upper bound on bytes read from a manifest or request file (4 MiB).
pub const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// A request older than this is refused (10 minutes).
pub const MAX_REQUEST_AGE_MS: u64 = 10 * 60 * 1000;

/// The complete handoff manifest written before the new binary is exec'd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffManifest {
    pub schema_version: u32,
    pub handoff_id: String,
    pub from_version: String,
    pub to_version: String,
    pub to_sha256: String,
    pub created_at_unix_ms: u64,
    pub data_fd: i32,
    pub tcp_fd: Option<i32>,
    pub proxy_started_at_unix_ms: u64,
    pub sessions: Vec<SessionSnapshot>,
    pub auth: Vec<AuthTokenSnapshot>,
}

/// One carried-over UDP session.
///
/// `pending_forward_us` is deliberately absent: it is zeroed at handoff so the
/// first response after adoption cannot be attributed a bogus latency spike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub client_addr: String,
    pub game_server: String,
    pub outbound_fd: i32,
    pub fec_enabled: bool,
    pub fec_k: u8,
    pub age_us: u64,
    pub idle_us: u64,
    pub response_seq: u16,
    pub last_client_seq: u16,
    pub packets_relayed: u64,
    pub bytes_relayed: u64,
}

/// One carried-over auth token.  `bound_port == 0` means "no port binding".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTokenSnapshot {
    pub token: u32,
    pub principal: String,
    pub bound_port: u16,
    pub ttl_ms_remaining: u64,
}

/// A request for the outgoing process to hand off to another binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffRequest {
    pub schema_version: u32,
    pub handoff_id: String,
    pub version: String,
    pub binary_path: String,
    pub sha256: String,
    pub requested_at_unix_ms: u64,
}

/// The outcome of the most recent handoff attempt, surfaced through `/health`.
///
/// `result` is one of [`RESULT_OK`], [`RESULT_REJECTED`], or [`RESULT_FAILED`].
/// `handoff_id`, `from_version`, and `to_version` are absent only when the
/// request was refused before those fields were known. `error` is present for
/// every non-`ok` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffStatus {
    pub schema_version: u32,
    pub supported: bool,
    pub handoff_id: Option<String>,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub result: String,
    pub sessions_transferred: u64,
    pub at_unix_ms: u64,
    pub error: Option<String>,
}

/// The `/health` view of handoff support: whether it is available, and the last
/// recorded status (or `null` when this process has not attempted one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffHealth {
    pub supported: bool,
    pub last: Option<HandoffStatus>,
}

impl HandoffStatus {
    fn base(result: &str, at_unix_ms: u64) -> Self {
        Self {
            schema_version: HANDOFF_SCHEMA_VERSION,
            supported: SUPPORTED,
            handoff_id: None,
            from_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            to_version: None,
            result: result.to_string(),
            sessions_transferred: 0,
            at_unix_ms,
            error: None,
        }
    }

    /// A request refused before it was acted on.
    pub fn rejected(error: impl Into<String>, at_unix_ms: u64) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::base(RESULT_REJECTED, at_unix_ms)
        }
    }

    /// A handoff that failed after acceptance and left the process serving.
    pub fn failed(error: impl Into<String>, at_unix_ms: u64) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::base(RESULT_FAILED, at_unix_ms)
        }
    }

    /// A handoff that completed and installed `sessions_transferred` sessions.
    pub fn ok(
        handoff_id: &str,
        from_version: &str,
        to_version: &str,
        sessions_transferred: u64,
        at_unix_ms: u64,
    ) -> Self {
        Self {
            schema_version: HANDOFF_SCHEMA_VERSION,
            supported: SUPPORTED,
            handoff_id: Some(handoff_id.to_string()),
            from_version: Some(from_version.to_string()),
            to_version: Some(to_version.to_string()),
            result: RESULT_OK.to_string(),
            sessions_transferred,
            at_unix_ms,
            error: None,
        }
    }

    /// Attach the request's identity to a rejection, when one was parsed.
    pub fn with_request(mut self, req: &HandoffRequest) -> Self {
        self.handoff_id = Some(req.handoff_id.clone());
        self.to_version = Some(req.version.clone());
        self
    }

    /// Attach the manifest's identity to a post-acceptance failure.
    pub fn with_manifest(mut self, manifest: &HandoffManifest) -> Self {
        self.handoff_id = Some(manifest.handoff_id.clone());
        self.to_version = Some(manifest.to_version.clone());
        self
    }
}

/// The last handoff status recorded in this process.
static STATUS: Mutex<Option<HandoffStatus>> = Mutex::new(None);

/// Record a handoff result in memory and, best effort, on disk.
///
/// The in-memory copy is always written so `/health` reflects the attempt even
/// when the result directory is missing or read-only. A file write failure is
/// swallowed: it must never fail `/health` or abort a handoff.
pub fn record_handoff_status(status: HandoffStatus) {
    *STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(status.clone());
    let path = result_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = write_json_atomic(&path, &status);
}

/// The last recorded handoff status, or `None` when none was attempted.
pub fn current_handoff_status() -> Option<HandoffStatus> {
    STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The `/health` handoff view for the current process.
pub fn handoff_health() -> HandoffHealth {
    HandoffHealth {
        supported: SUPPORTED,
        last: current_handoff_status(),
    }
}

/// Drop the recorded status. Tests that repoint the result path call this.
#[cfg(test)]
pub(crate) fn reset_handoff_status_for_test() {
    *STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Resolve the manifest write path (`LIGHTSPEED_HANDOFF_MANIFEST` override).
pub fn manifest_write_path() -> PathBuf {
    match std::env::var_os(MANIFEST_ENV) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_MANIFEST_PATH),
    }
}

/// The manifest path named by the adoption trigger, if the env var is set.
pub fn manifest_path_from_env() -> Option<PathBuf> {
    match std::env::var_os(MANIFEST_ENV) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

/// Resolve the release root (`LIGHTSPEED_RELEASES_DIR` override).
pub fn releases_root() -> PathBuf {
    match std::env::var_os(RELEASES_DIR_ENV) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_RELEASES_DIR),
    }
}

/// Resolve the result path (`LIGHTSPEED_HANDOFF_RESULT` override).
pub fn result_path() -> PathBuf {
    match std::env::var_os(RESULT_PATH_ENV) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_RESULT_PATH),
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A process-unique handoff id built from the current time and the pid.
pub fn new_handoff_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:032x}{:08x}", std::process::id())
}

/// Write `value` as JSON to `path`, atomically.
///
/// The bytes go to a fresh temp file in the same directory (mode 0600 on Unix),
/// are fsync'd, then renamed over `path`, so a reader never observes a partial
/// document and the manifest is never group/world readable.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent directory: {}", path.display()))?;
    let stem = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("path has no file name: {}", path.display()))?;

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = parent.join(format!(
        ".{}.tmp.{}.{}",
        stem.to_string_lossy(),
        std::process::id(),
        seq
    ));

    let write_result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

fn read_capped(path: &Path, cap: u64) -> anyhow::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    std::io::BufReader::new(file)
        .take(cap + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > cap {
        anyhow::bail!("file {} exceeds the {} byte cap", path.display(), cap);
    }
    Ok(buf)
}

/// Read and validate a handoff manifest, enforcing [`MAX_MANIFEST_BYTES`] and
/// the expected [`HANDOFF_SCHEMA_VERSION`].
pub fn read_manifest(path: &Path) -> anyhow::Result<HandoffManifest> {
    let buf = read_capped(path, MAX_MANIFEST_BYTES)?;
    let manifest: HandoffManifest = serde_json::from_slice(&buf)?;
    if manifest.schema_version != HANDOFF_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported handoff manifest schema_version {} (expected {})",
            manifest.schema_version,
            HANDOFF_SCHEMA_VERSION
        );
    }
    Ok(manifest)
}

/// Read and validate a handoff request, enforcing [`MAX_MANIFEST_BYTES`] and
/// the expected [`HANDOFF_SCHEMA_VERSION`].
pub fn read_request(path: &Path) -> anyhow::Result<HandoffRequest> {
    let buf = read_capped(path, MAX_MANIFEST_BYTES)?;
    let request: HandoffRequest = serde_json::from_slice(&buf)?;
    if request.schema_version != HANDOFF_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported handoff request schema_version {} (expected {})",
            request.schema_version,
            HANDOFF_SCHEMA_VERSION
        );
    }
    Ok(request)
}

// ─────────────────────────────────────────────────────────────────────────────
//  File-descriptor transfer (Linux)
// ─────────────────────────────────────────────────────────────────────────────

/// Clear `FD_CLOEXEC` so `fd` survives an `execve`.
#[cfg(target_os = "linux")]
pub fn clear_cloexec(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    set_fd_flag(fd, false)
}

/// Set `FD_CLOEXEC` so `fd` is closed on `execve`.
#[cfg(target_os = "linux")]
pub fn set_cloexec(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    set_fd_flag(fd, true)
}

/// Close a raw fd that is being abandoned before adoption.
///
/// Only call this for a descriptor this process still owns and has not yet
/// handed to a wrapper. Closing a descriptor already owned by a [`std::net::UdpSocket`]
/// (or any RAII owner) would double-close it.
#[cfg(target_os = "linux")]
pub fn close_fd(fd: std::os::fd::RawFd) {
    if fd < 0 {
        return;
    }
    // SAFETY: `close` only releases the descriptor table entry `fd`. An invalid
    // or already-closed descriptor returns `EBADF` without touching memory.
    unsafe {
        libc::close(fd);
    }
}

#[cfg(target_os = "linux")]
fn set_fd_flag(fd: std::os::fd::RawFd, cloexec: bool) -> std::io::Result<()> {
    // SAFETY: `fcntl(F_GETFD)` only reads the descriptor flags of `fd` and has
    // no memory-safety preconditions.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let updated = if cloexec {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: `fcntl(F_SETFD)` only writes the descriptor flags of `fd`.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, updated) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Whether `fd` has `FD_CLOEXEC` set.
#[cfg(target_os = "linux")]
pub fn is_cloexec(fd: std::os::fd::RawFd) -> std::io::Result<bool> {
    // SAFETY: `fcntl(F_GETFD)` only reads the descriptor flags of `fd`.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(flags & libc::FD_CLOEXEC != 0)
}

/// Validate that `fd` is a usable IPv4 UDP socket.
///
/// `fstat` must report a socket, the socket family must be `AF_INET`, its type
/// must be `SOCK_DGRAM`, and `getsockname` must succeed.
#[cfg(target_os = "linux")]
pub fn validate_udp_fd(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    if fd < 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "negative file descriptor",
        ));
    }

    // SAFETY: `st` is zeroed and `fstat` writes at most `size_of::<stat>()`
    // bytes into it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(Error::last_os_error());
    }
    if st.st_mode & libc::S_IFMT != libc::S_IFSOCK {
        return Err(Error::new(ErrorKind::InvalidInput, "fd is not a socket"));
    }

    // SAFETY: `ss` is zeroed and `getsockname` writes at most `len` bytes into
    // it; `len` is initialised to the full storage size.
    let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: the cast is a `sockaddr_storage` pointer used as the generic
    // `sockaddr` out-parameter with its matching length.
    let rc = unsafe {
        libc::getsockname(
            fd,
            &mut ss as *mut libc::sockaddr_storage as *mut libc::sockaddr,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(Error::last_os_error());
    }
    if ss.ss_family as libc::c_int != libc::AF_INET {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "fd is not an AF_INET socket",
        ));
    }

    let mut sock_type: libc::c_int = 0;
    let mut optlen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `sock_type`/`optlen` are correctly sized for SO_TYPE.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            &mut sock_type as *mut libc::c_int as *mut libc::c_void,
            &mut optlen,
        )
    };
    if rc != 0 {
        return Err(Error::last_os_error());
    }
    if sock_type != libc::SOCK_DGRAM {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "fd is not a SOCK_DGRAM socket",
        ));
    }
    Ok(())
}

/// Adopt a validated raw fd as a non-blocking [`std::net::UdpSocket`].
///
/// Validation runs first; only a validated fd is taken ownership of.
#[cfg(target_os = "linux")]
pub fn adopt_std_udp(fd: std::os::fd::RawFd) -> std::io::Result<std::net::UdpSocket> {
    validate_udp_fd(fd)?;
    // SAFETY: `validate_udp_fd` proved `fd` is a live UDP socket, and the caller
    // transfers ownership of it (the returned socket owns and closes it).
    let socket = unsafe { <std::net::UdpSocket as std::os::fd::FromRawFd>::from_raw_fd(fd) };
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Lowercase-hex SHA-256 of every byte read from `reader`.
pub fn sha256_reader(mut reader: impl Read) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Lowercase-hex SHA-256 of the file at `path`.
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    Ok(sha256_reader(&mut file)?)
}

/// Open the requested binary and bind every check to the exact inode that will
/// be executed, closing the hash-then-execute TOCTOU window.
///
/// `std::fs::File::open` opens `O_RDONLY | O_CLOEXEC`, so the returned handle
/// owns a private descriptor. The descriptor is `fstat`-checked (regular file,
/// not group/world writable, root-owned) and the SHA-256 is computed from the
/// same descriptor's contents, never re-resolved by path. The caller must clear
/// `FD_CLOEXEC` on [`std::os::fd::AsRawFd::as_raw_fd`] and execute
/// `/proc/self/fd/<fd>` so the kernel runs the verified inode, not a path that
/// could be swapped after the check. On any failure the returned `Err` is a
/// refusal: callers must never fall back to executing the raw path.
#[cfg(target_os = "linux")]
pub fn open_verified_binary(req: &HandoffRequest) -> Result<(std::fs::File, String), String> {
    use std::os::unix::fs::MetadataExt;

    let mut file = std::fs::File::open(&req.binary_path)
        .map_err(|e| format!("cannot open binary_path {}: {e}", req.binary_path))?;

    let metadata = file
        .metadata()
        .map_err(|e| format!("binary metadata: {e}"))?;
    if !metadata.is_file() {
        return Err("binary_path is not a regular file".to_string());
    }
    if metadata.mode() & 0o022 != 0 {
        return Err("binary is group/world writable".to_string());
    }

    let actual = sha256_reader(&mut file).map_err(|e| format!("cannot hash binary: {e}"))?;
    if !actual.eq_ignore_ascii_case(&req.sha256) {
        return Err(format!(
            "sha256 mismatch: expected {}, got {actual}",
            req.sha256
        ));
    }
    if metadata.uid() != 0 {
        return Err("binary is not root-owned".to_string());
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("cannot rewind verified binary: {e}"))?;
    Ok((file, actual))
}

/// Validate an untrusted [`HandoffRequest`] before acting on it.
///
/// Enforces, in order: schema version, absolute `binary_path` with no `..`,
/// canonicalization inside `release_root`, a regular file, a mode without
/// group/world write bits, a matching SHA-256, an age under ten minutes, and
/// root ownership.
#[cfg(target_os = "linux")]
pub fn validate_request(
    req: &HandoffRequest,
    release_root: &Path,
    now_unix_ms: u64,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;

    if req.schema_version != HANDOFF_SCHEMA_VERSION {
        return Err(format!(
            "unsupported request schema_version {}",
            req.schema_version
        ));
    }

    let binary = Path::new(&req.binary_path);
    if !binary.is_absolute() {
        return Err("binary_path must be absolute".to_string());
    }
    if binary
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("binary_path must not contain '..'".to_string());
    }

    let root = release_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize release_root: {e}"))?;
    let resolved = binary
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize binary_path: {e}"))?;
    if !resolved.starts_with(&root) {
        return Err(format!(
            "binary_path {} is outside release_root {}",
            resolved.display(),
            root.display()
        ));
    }

    let metadata = std::fs::metadata(&resolved).map_err(|e| format!("binary metadata: {e}"))?;
    if !metadata.is_file() {
        return Err("binary_path is not a regular file".to_string());
    }
    if metadata.mode() & 0o022 != 0 {
        return Err("binary is group/world writable".to_string());
    }

    let actual = sha256_file(&resolved).map_err(|e| format!("cannot hash binary: {e}"))?;
    if !actual.eq_ignore_ascii_case(&req.sha256) {
        return Err(format!(
            "sha256 mismatch: expected {}, got {actual}",
            req.sha256
        ));
    }

    if now_unix_ms.saturating_sub(req.requested_at_unix_ms) > MAX_REQUEST_AGE_MS {
        return Err("handoff request is older than 10 minutes".to_string());
    }

    if metadata.uid() != 0 {
        return Err("binary is not root-owned".to_string());
    }

    Ok(())
}

/// Decide whether this running binary is the target the manifest names.
///
/// Enforces the manifest schema, that `to_version` is the running package
/// version, and that `to_sha256` matches `self_sha256`. The caller supplies the
/// running version and the hash of its own executable so the decision stays
/// pure and unit-testable without `/proc/self/exe`.
pub fn check_adoption_identity(
    manifest: &HandoffManifest,
    running_version: &str,
    self_sha256: &str,
) -> Result<(), String> {
    if manifest.schema_version != HANDOFF_SCHEMA_VERSION {
        return Err(format!(
            "unsupported manifest schema_version {} (expected {})",
            manifest.schema_version, HANDOFF_SCHEMA_VERSION
        ));
    }
    if manifest.to_version != running_version {
        return Err(format!(
            "manifest to_version {} does not match running version {running_version}",
            manifest.to_version
        ));
    }
    if !manifest.to_sha256.eq_ignore_ascii_case(self_sha256) {
        return Err(format!(
            "manifest to_sha256 {} does not match running executable sha256 {self_sha256}",
            manifest.to_sha256
        ));
    }
    Ok(())
}

/// Validate the data fd and every carried session's outbound fd.
///
/// The adopting process runs this before taking ownership of any fd, so a
/// manifest that names a regular file, a closed fd, a non-UDP socket, or the
/// same fd twice is refused without partial adoption. A duplicate fd would be
/// adopted as two owners and double-closed.
#[cfg(target_os = "linux")]
pub fn validate_manifest_fds(manifest: &HandoffManifest) -> Result<(), String> {
    use std::collections::HashSet;

    let mut seen: HashSet<std::os::fd::RawFd> = HashSet::with_capacity(1 + manifest.sessions.len());
    if !seen.insert(manifest.data_fd) {
        return Err(format!(
            "duplicate fd {} in handoff manifest",
            manifest.data_fd
        ));
    }
    validate_udp_fd(manifest.data_fd).map_err(|e| format!("data_fd {}: {e}", manifest.data_fd))?;
    for snap in &manifest.sessions {
        if !seen.insert(snap.outbound_fd) {
            return Err(format!(
                "duplicate fd {} in handoff manifest (outbound_fd for client {})",
                snap.outbound_fd, snap.client_addr
            ));
        }
        validate_udp_fd(snap.outbound_fd).map_err(|e| {
            format!(
                "outbound_fd {} for client {}: {e}",
                snap.outbound_fd, snap.client_addr
            )
        })?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "ls-handoff-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_manifest(schema_version: u32) -> HandoffManifest {
        HandoffManifest {
            schema_version,
            handoff_id: "0123456789abcdef".to_string(),
            from_version: "1.4.4".to_string(),
            to_version: "1.4.5".to_string(),
            to_sha256: "abc123".to_string(),
            created_at_unix_ms: 1_700_000_000_000,
            data_fd: 7,
            tcp_fd: Some(8),
            proxy_started_at_unix_ms: 1_699_999_000_000,
            sessions: vec![SessionSnapshot {
                client_addr: "203.0.113.5:40000".to_string(),
                game_server: "198.51.100.9:27015".to_string(),
                outbound_fd: 11,
                fec_enabled: true,
                fec_k: 4,
                age_us: 12_345_678,
                idle_us: 123_456,
                response_seq: 42,
                last_client_seq: 99,
                packets_relayed: 1000,
                bytes_relayed: 500_000,
            }],
            auth: vec![AuthTokenSnapshot {
                token: 0xdead_beef,
                principal: "203.0.113.5".to_string(),
                bound_port: 40000,
                ttl_ms_remaining: 120_000,
            }],
        }
    }

    #[cfg(target_os = "linux")]
    fn sample_session(outbound_fd: i32, client_addr: &str) -> SessionSnapshot {
        SessionSnapshot {
            client_addr: client_addr.to_string(),
            game_server: "198.51.100.9:27015".to_string(),
            outbound_fd,
            fec_enabled: false,
            fec_k: 4,
            age_us: 1,
            idle_us: 1,
            response_seq: 0,
            last_client_seq: 0,
            packets_relayed: 0,
            bytes_relayed: 0,
        }
    }

    fn sample_request() -> HandoffRequest {
        HandoffRequest {
            schema_version: HANDOFF_SCHEMA_VERSION,
            handoff_id: "0123456789abcdef".to_string(),
            version: "1.4.5".to_string(),
            binary_path: "/opt/lightspeed/releases/1.4.5/lightspeed-proxy".to_string(),
            sha256: "0".repeat(64),
            requested_at_unix_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn manifest_round_trips_through_disk() {
        let dir = temp_dir("manifest");
        let path = dir.join("handoff.json");
        let manifest = sample_manifest(HANDOFF_SCHEMA_VERSION);

        write_json_atomic(&path, &manifest).unwrap();
        let read_back = read_manifest(&path).unwrap();

        assert_eq!(read_back, manifest);
    }

    #[test]
    fn request_round_trips_through_disk() {
        let dir = temp_dir("request");
        let path = dir.join("handoff-request.json");
        let request = sample_request();

        write_json_atomic(&path, &request).unwrap();
        let read_back = read_request(&path).unwrap();

        assert_eq!(read_back, request);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_creates_a_private_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("private");
        let path = dir.join("handoff.json");
        write_json_atomic(&path, &sample_request()).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "manifest must be owner-only");
    }

    #[test]
    fn read_manifest_rejects_oversized_file() {
        let dir = temp_dir("oversized");
        let path = dir.join("handoff.json");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_MANIFEST_BYTES + 1).unwrap();
        drop(file);

        assert!(read_manifest(&path).is_err());
    }

    #[test]
    fn read_manifest_rejects_wrong_schema() {
        let dir = temp_dir("schema");
        let path = dir.join("handoff.json");
        write_json_atomic(&path, &sample_manifest(999)).unwrap();

        assert!(read_manifest(&path).is_err());
    }

    #[test]
    fn read_manifest_rejects_malformed_json() {
        let dir = temp_dir("malformed");
        let path = dir.join("handoff.json");
        std::fs::write(&path, b"{ this is not json").unwrap();

        assert!(read_manifest(&path).is_err());
    }

    #[test]
    fn read_request_rejects_oversized_file() {
        let dir = temp_dir("req-oversized");
        let path = dir.join("handoff-request.json");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_MANIFEST_BYTES + 1).unwrap();
        drop(file);

        assert!(read_request(&path).is_err());
    }

    #[test]
    fn read_request_rejects_wrong_schema() {
        let dir = temp_dir("req-schema");
        let path = dir.join("handoff-request.json");
        let mut request = sample_request();
        request.schema_version = 999;
        write_json_atomic(&path, &request).unwrap();

        assert!(read_request(&path).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn validate_udp_fd_accepts_a_udp_socket_and_rejects_a_file() {
        use std::os::fd::AsRawFd;

        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        assert!(validate_udp_fd(socket.as_raw_fd()).is_ok());

        let dir = temp_dir("fd-file");
        let path = dir.join("regular.txt");
        std::fs::write(&path, b"not a socket").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        assert!(validate_udp_fd(file.as_raw_fd()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cloexec_round_trips_on_a_socket() {
        use std::os::fd::AsRawFd;

        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let fd = socket.as_raw_fd();

        clear_cloexec(fd).unwrap();
        assert!(!is_cloexec(fd).unwrap());
        set_cloexec(fd).unwrap();
        assert!(is_cloexec(fd).unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn adopt_std_udp_preserves_the_bound_address() {
        use std::os::fd::AsRawFd;

        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        let fd = socket.as_raw_fd();

        // Transfer ownership of the fd to `adopt_std_udp`; forgetting the
        // original wrapper prevents a double close.
        std::mem::forget(socket);
        let adopted = adopt_std_udp(fd).unwrap();

        assert_eq!(adopted.local_addr().unwrap(), addr);
    }

    #[test]
    fn sha256_file_matches_a_known_digest() {
        let dir = temp_dir("sha");
        let path = dir.join("payload.bin");
        std::fs::write(&path, b"abc").unwrap();

        // SHA-256("abc")
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn close_fd_releases_a_descriptor_without_affecting_others() {
        close_fd(-1);

        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `pipe` writes two valid descriptors into the array.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (read_end, write_end) = (fds[0], fds[1]);

        close_fd(write_end);

        let mut buf = [0u8; 1];
        // SAFETY: `read_end` is a live descriptor and `buf` has capacity 1.
        let n = unsafe { libc::read(read_end, buf.as_mut_ptr() as *mut libc::c_void, 1) };
        assert_eq!(n, 0, "closing the write end makes the read end report EOF");
        // SAFETY: `read_end` is still owned by this test.
        unsafe { libc::close(read_end) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_verified_binary_binds_its_checks_to_the_opened_fd() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("verify-bin");
        let path = dir.join("lightspeed-proxy");
        std::fs::write(&path, b"#!/bin/true\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut request = sample_request();
        request.binary_path = path.to_string_lossy().into_owned();

        let err = open_verified_binary(&request).unwrap_err();
        assert!(err.contains("sha256"), "unexpected error: {err}");

        if unsafe { libc::geteuid() } != 0 {
            request.sha256 = sha256_file(&path).unwrap();
            let err = open_verified_binary(&request).unwrap_err();
            assert!(err.contains("root-owned"), "unexpected error: {err}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn validate_manifest_fds_rejects_duplicate_fds() {
        use std::os::fd::AsRawFd;

        let data = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let out_a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let out_b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let data_fd = data.as_raw_fd();
        let a_fd = out_a.as_raw_fd();
        let b_fd = out_b.as_raw_fd();

        let mut manifest = sample_manifest(HANDOFF_SCHEMA_VERSION);
        manifest.data_fd = data_fd;
        manifest.sessions = vec![
            sample_session(a_fd, "203.0.113.5:40000"),
            sample_session(b_fd, "203.0.113.6:40001"),
        ];
        assert!(validate_manifest_fds(&manifest).is_ok());

        manifest.sessions[1].outbound_fd = a_fd;
        let err = validate_manifest_fds(&manifest).unwrap_err();
        assert!(err.contains("duplicate"), "unexpected error: {err}");

        manifest.sessions[1].outbound_fd = data_fd;
        let err = validate_manifest_fds(&manifest).unwrap_err();
        assert!(err.contains("duplicate"), "unexpected error: {err}");
    }

    /// Serializes env-var mutation across parallel tests.
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn adoption_identity_rejects_wrong_schema_version_and_sha() {
        let good = sample_manifest(HANDOFF_SCHEMA_VERSION);
        assert!(check_adoption_identity(&good, "1.4.5", "ABC123").is_ok());

        let mut wrong_schema = good.clone();
        wrong_schema.schema_version = 999;
        assert!(check_adoption_identity(&wrong_schema, "1.4.5", "ABC123").is_err());

        let mut wrong_version = good.clone();
        wrong_version.to_version = "9.9.9".to_string();
        assert!(check_adoption_identity(&wrong_version, "1.4.5", "ABC123").is_err());

        let mut wrong_sha = good;
        wrong_sha.to_sha256 = "deadbeef".to_string();
        assert!(check_adoption_identity(&wrong_sha, "1.4.5", "ABC123").is_err());
    }

    #[test]
    fn handoff_status_records_to_memory_and_file() {
        let _guard = env_guard();
        reset_handoff_status_for_test();
        let dir = temp_dir("status");
        let path = dir.join("handoff-result.json");
        std::env::set_var(RESULT_PATH_ENV, &path);

        record_handoff_status(HandoffStatus::ok(
            "id1",
            "1.4.4",
            "1.4.5",
            3,
            1_700_000_000_000,
        ));
        let current = current_handoff_status().expect("status recorded");
        assert_eq!(current.result, RESULT_OK);
        assert_eq!(current.sessions_transferred, 3);
        assert_eq!(current.supported, SUPPORTED);
        assert_eq!(current.at_unix_ms, 1_700_000_000_000);

        let bytes = std::fs::read(&path).expect("result file written");
        let on_disk: HandoffStatus = serde_json::from_slice(&bytes).expect("valid result JSON");
        assert_eq!(on_disk, current);

        let health = handoff_health();
        assert_eq!(health.supported, SUPPORTED);
        assert_eq!(health.last, Some(current));

        std::env::remove_var(RESULT_PATH_ENV);
        reset_handoff_status_for_test();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejected_status_carries_request_identity() {
        let status = HandoffStatus::rejected("bad request", 42).with_request(&sample_request());
        assert_eq!(status.result, RESULT_REJECTED);
        assert_eq!(status.handoff_id.as_deref(), Some("0123456789abcdef"));
        assert_eq!(status.to_version.as_deref(), Some("1.4.5"));
        assert_eq!(status.error.as_deref(), Some("bad request"));
        assert_eq!(status.sessions_transferred, 0);
    }

    #[cfg(target_os = "linux")]
    mod request_validation {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn valid_request_for(
            binary: &Path,
            sha256: &str,
            requested_at_unix_ms: u64,
        ) -> HandoffRequest {
            HandoffRequest {
                schema_version: HANDOFF_SCHEMA_VERSION,
                handoff_id: "0123456789abcdef".to_string(),
                version: "1.4.5".to_string(),
                binary_path: binary.to_string_lossy().into_owned(),
                sha256: sha256.to_string(),
                requested_at_unix_ms,
            }
        }

        fn release_layout(tag: &str) -> (PathBuf, PathBuf) {
            let base = temp_dir(tag);
            let root = base.join("release");
            let bin_dir = root.join("1.4.5");
            std::fs::create_dir_all(&bin_dir).unwrap();
            (root, bin_dir)
        }

        fn write_binary(dir: &Path, mode: u32) -> PathBuf {
            let bin = dir.join("lightspeed-proxy");
            std::fs::write(&bin, b"#!/bin/true\n").unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(mode)).unwrap();
            bin
        }

        #[test]
        fn rejects_a_binary_outside_the_release_root() {
            let base = temp_dir("outside");
            let root = base.join("release");
            let other = base.join("other");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&other).unwrap();
            let bin = write_binary(&other, 0o644);
            let sha = sha256_file(&bin).unwrap();
            let now = 1_700_000_000_000;

            let request = valid_request_for(&bin, &sha, now);
            let err = validate_request(&request, &root, now).unwrap_err();
            assert!(err.contains("outside"), "unexpected error: {err}");
        }

        #[test]
        fn rejects_a_sha256_mismatch() {
            let (root, bin_dir) = release_layout("sha-mismatch");
            let bin = write_binary(&bin_dir, 0o644);
            let now = 1_700_000_000_000;

            let request = valid_request_for(&bin, &"f".repeat(64), now);
            let err = validate_request(&request, &root, now).unwrap_err();
            assert!(err.contains("sha256"), "unexpected error: {err}");
        }

        #[test]
        fn rejects_a_stale_request() {
            let (root, bin_dir) = release_layout("stale");
            let bin = write_binary(&bin_dir, 0o644);
            let sha = sha256_file(&bin).unwrap();
            let now = 1_700_000_000_000;

            let request = valid_request_for(&bin, &sha, now - MAX_REQUEST_AGE_MS - 1);
            let err = validate_request(&request, &root, now).unwrap_err();
            assert!(err.contains("older"), "unexpected error: {err}");
        }

        #[test]
        fn rejects_a_group_or_world_writable_binary() {
            let (root, bin_dir) = release_layout("writable");
            let bin = write_binary(&bin_dir, 0o666);
            let sha = sha256_file(&bin).unwrap();
            let now = 1_700_000_000_000;

            let request = valid_request_for(&bin, &sha, now);
            let err = validate_request(&request, &root, now).unwrap_err();
            assert!(err.contains("writable"), "unexpected error: {err}");
        }

        #[test]
        fn rejects_a_relative_binary_path() {
            let (root, _bin_dir) = release_layout("relative");
            let now = 1_700_000_000_000;

            let mut request = valid_request_for(Path::new("/unused"), &"0".repeat(64), now);
            request.binary_path = "lightspeed-proxy".to_string();
            assert!(validate_request(&request, &root, now).is_err());
        }
    }
}
