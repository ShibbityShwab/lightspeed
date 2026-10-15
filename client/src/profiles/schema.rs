//! User-profile schema (v1): custom games and saved servers.
//!
//! One TOML document - `profiles.toml` next to the client config dir (see
//! [`crate::paths`]):
//!
//! ```toml
//! version = 1
//!
//! [[games]]
//! name = "My Game"                    # required, display name
//! key = "my-game"                     # optional; slug of `name` when absent
//! process_names = ["MyGame.exe"]      # optional; enables detection/targeting
//! udp_ports = "27015-27050"           # optional; "lo-hi" or a single port
//! dynamic_server = false              # optional; rotating servers
//! notes = "Community server uses UDP 27015"  # optional
//!
//! [[servers]]
//! name = "My community server"
//! address = "203.0.113.7:27015"
//! udp_ports = "27015-27050"           # optional
//! ```
//!
//! Every rule lives in this one file, so the load path (tolerant, reporting)
//! and the save path (strict) cannot drift apart. They are the same checks
//! with different severities:
//!
//! - **Unknown keys are ignored** (forward compatibility); malformed entries
//!   are skipped **and reported**, matching the tolerant-but-visible style of
//!   the GUI config loader.
//! - **`tcp_ports` and `pid` are rejected** with a reason that names the key.
//!   The interception path is UDP-only, so a TCP port list would never reach a
//!   filter; a stored pid does not survive a restart or a PID reuse. Refusing
//!   the key beats honouring it in name only.
//! - Port ranges are `1..=65535` with `lo <= hi`, reusing the GUI's
//!   `parse_custom_port_range` shape (trimmed, `"lo-hi"` or a single port, a
//!   blank value means "no range"); the difference is that a reason comes back
//!   instead of a silent `None`, because a skipped entry has to be explained.
//! - At least one of `process_names` / `udp_ports` per game.
//! - Caps: [`MAX_GAMES`] games, [`MAX_SERVERS`] servers. Entries past a cap
//!   are skipped and reported; a save that would exceed one is a hard error.
//! - A `key`, or a `process_names` entry, that a built-in already claims is a
//!   load-time warning and a save-time error, and both **name the built-in
//!   that won**. Built-ins outrank user claims at resolve time, so the loader
//!   keeps the entry while the saver refuses to write it. Shadowing is never
//!   silent.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::SocketAddrV4;

/// Maximum number of `[[games]]` entries kept from one file.
pub const MAX_GAMES: usize = 64;
/// Maximum number of `[[servers]]` entries kept from one file.
pub const MAX_SERVERS: usize = 64;
/// Longest allowed profile key: `[a-z0-9-]{1,24}`.
pub const KEY_MAX_LEN: usize = 24;
/// Diagnostic entry label for problems that belong to the whole document.
const FILE: &str = "file";

/// Keys the schema refuses outright, with the reason printed for each.
///
/// Presence is what matters, not the value: the loader looks for the key on the
/// raw table, so `tcp_ports = "25565"` and `tcp_ports = 0` are refused alike.
const REJECTED_KEYS: &[(&str, &str)] = &[
    (
        "tcp_ports",
        "the interception path is UDP-only, so a TCP port list would never reach a filter; \
         refusing the key beats honouring it in name only (TCP in process mode is a later phase)",
    ),
    (
        "pid",
        "a stored pid does not survive a restart and is reused by other processes; \
         process identity comes from process_names plus a fresh scan",
    ),
];

/// A validated UDP port range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub lo: u16,
    pub hi: u16,
}

impl PortRange {
    /// Build a range, enforcing the schema's `1..=65535`, `lo <= hi` bounds.
    pub fn new(lo: u16, hi: u16) -> Result<Self, String> {
        if lo == 0 || hi == 0 {
            return Err(format!(
                "port 0 is not a valid port (the range is 1-65535): {lo}-{hi}"
            ));
        }
        if lo > hi {
            return Err(format!("'{lo}-{hi}' has lo > hi"));
        }
        Ok(Self { lo, hi })
    }
}

impl fmt::Display for PortRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.lo == self.hi {
            write!(f, "{}", self.lo)
        } else {
            write!(f, "{}-{}", self.lo, self.hi)
        }
    }
}

/// Parse a user-supplied port range string.
///
/// Same shape as the GUI's `parse_custom_port_range`
/// (`client-gui/src/app.rs`): trimmed, `"lo-hi"` or a single port, blank means
/// "no range". On top of it, the schema's `1..=65535` bound and a reason for
/// every rejection, because a skipped entry must be explained.
pub fn parse_port_range(s: &str) -> Result<Option<PortRange>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if let Some((lo_s, hi_s)) = s.split_once('-') {
        let lo = parse_port(lo_s, s)?;
        let hi = parse_port(hi_s, s)?;
        PortRange::new(lo, hi).map(Some)
    } else {
        let p = parse_port(s, s)?;
        PortRange::new(p, p).map(Some)
    }
}

fn parse_port(s: &str, whole: &str) -> Result<u16, String> {
    s.trim().parse::<u16>().map_err(|_| {
        format!("'{whole}' is not a port range (expected \"lo-hi\" or one port 1-65535)")
    })
}

/// Whether `key` matches the schema's `[a-z0-9-]{1,24}`.
pub fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= KEY_MAX_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Derive a profile key from a display name: lowercase, runs of anything that
/// is not `[a-z0-9]` collapsed to a single `-`, trimmed to 24 chars. `None`
/// when nothing survives (a name with no ASCII alphanumerics).
pub fn slugify(name: &str) -> Option<String> {
    let mut out = String::new();
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            out.push(lower);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > KEY_MAX_LEN {
        out.truncate(KEY_MAX_LEN);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// A user-defined game: one `[[games]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGameProfile {
    pub name: String,
    /// Resolved key: the explicit `key`, else the slug of `name`.
    pub key: String,
    pub process_names: Vec<String>,
    pub udp_ports: Option<PortRange>,
    pub dynamic_server: bool,
    pub notes: Option<String>,
}

/// A saved server preset: one `[[servers]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserServer {
    pub name: String,
    /// `ip:port`, validated before it is accepted.
    pub address: String,
    pub udp_ports: Option<PortRange>,
}

/// How the loader treated one problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The entry was dropped (malformed, invalid, or past a cap).
    Skipped,
    /// The entry was dropped because it carries a key the schema refuses.
    Rejected,
    /// The entry was kept, but a built-in outranks it. A save is a hard error.
    Warning,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Skipped => "skipped",
            Severity::Rejected => "rejected",
            Severity::Warning => "warning",
        }
    }
}

/// One reported problem, naming the entry it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// `games[3]`, `servers[0]`, or `file`.
    pub entry: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(severity: Severity, entry: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            entry: entry.into(),
            message: message.into(),
        }
    }

    pub fn skipped(entry: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Skipped, entry, message)
    }

    pub fn rejected(entry: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Rejected, entry, message)
    }

    pub fn warning(entry: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, entry, message)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.entry,
            self.severity.label(),
            self.message
        )
    }
}

/// One problem found while saving: the whole-document view of a
/// [`Diagnostic`], plus the caps and the write failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveError {
    pub entry: String,
    pub message: String,
}

impl SaveError {
    pub fn new(entry: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            entry: entry.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.entry, self.message)
    }
}

impl std::error::Error for SaveError {}

/// A parsed document: what was kept, and what was reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedProfiles {
    pub version: Option<i64>,
    pub games: Vec<UserGameProfile>,
    pub servers: Vec<UserServer>,
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadedProfiles {
    pub fn is_empty(&self) -> bool {
        self.games.is_empty() && self.servers.is_empty()
    }

    /// Log every diagnostic at `warn` level: the "and reported" half of the
    /// tolerant loader. One line per problem, each naming its entry.
    pub fn report(&self) {
        for d in &self.diagnostics {
            tracing::warn!("profiles: {d}");
        }
    }
}

/// Parse a profiles document with load semantics: never fails, every problem
/// comes back as a [`Diagnostic`].
pub fn load_from_str(text: &str) -> LoadedProfiles {
    let mut out = LoadedProfiles::default();

    let doc: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            out.diagnostics
                .push(Diagnostic::skipped(FILE, format!("not valid TOML: {e}")));
            return out;
        }
    };
    let table = match doc.as_table() {
        Some(t) => t,
        None => {
            out.diagnostics.push(Diagnostic::skipped(
                FILE,
                format!("the document must be a TOML table, got {}", doc.type_str()),
            ));
            return out;
        }
    };

    out.version = match take_i64(table, "version") {
        Ok(v) => v,
        Err(e) => {
            out.diagnostics.push(Diagnostic::skipped(FILE, e));
            None
        }
    };

    let raw_games = take_entry_array(table, "games", &mut out.diagnostics);
    for (i, entry) in raw_games.iter().enumerate() {
        let label = format!("games[{i}]");
        if out.games.len() >= MAX_GAMES {
            out.diagnostics.push(Diagnostic::skipped(
                label,
                format!("beyond the {MAX_GAMES}-game cap, ignored"),
            ));
            continue;
        }
        match parse_game_entry(&label, entry) {
            Ok(game) => out.games.push(game),
            Err(diags) => out.diagnostics.extend(diags),
        }
    }

    let raw_servers = take_entry_array(table, "servers", &mut out.diagnostics);
    for (i, entry) in raw_servers.iter().enumerate() {
        let label = format!("servers[{i}]");
        if out.servers.len() >= MAX_SERVERS {
            out.diagnostics.push(Diagnostic::skipped(
                label,
                format!("beyond the {MAX_SERVERS}-server cap, ignored"),
            ));
            continue;
        }
        match parse_server_entry(&label, entry) {
            Ok(server) => out.servers.push(server),
            Err(diags) => out.diagnostics.extend(diags),
        }
    }

    out.diagnostics.extend(document_diagnostics(&out.games));
    out
}

/// Save-time validation of a document in text form.
///
/// Every problem the tolerant loader reports - skipped, rejected, or the
/// built-in-collision warning - is a hard error here, with the same message.
pub fn validate_text_for_save(text: &str) -> Result<LoadedProfiles, Vec<SaveError>> {
    let doc = load_from_str(text);
    let mut errors: Vec<SaveError> = doc
        .diagnostics
        .iter()
        .map(|d| SaveError::new(d.entry.clone(), d.message.clone()))
        .collect();
    errors.extend(typed_errors(&doc.games, &doc.servers));
    errors = dedup(errors);
    if errors.is_empty() {
        Ok(doc)
    } else {
        Err(errors)
    }
}

/// Save-time validation of typed values (the GUI editor's path, which never
/// produces a rejected key: the schema has no such field).
pub fn validate_for_save(
    games: &[UserGameProfile],
    servers: &[UserServer],
) -> Result<(), Vec<SaveError>> {
    let errors = typed_errors(games, servers);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Write a validated document, creating the parent directory when needed.
pub fn save_to_file(
    path: &std::path::Path,
    games: &[UserGameProfile],
    servers: &[UserServer],
) -> Result<(), Vec<SaveError>> {
    validate_for_save(games, servers)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(vec![SaveError::new(
                    path.display().to_string(),
                    format!("cannot create {}: {e}", parent.display()),
                )]);
            }
        }
    }
    std::fs::write(path, to_toml_string(games, servers)).map_err(|e| {
        vec![SaveError::new(
            path.display().to_string(),
            format!("cannot write: {e}"),
        )]
    })
}

/// Serialize a document. The writer emits only schema keys, so a document that
/// was loaded with a rejected key (`tcp_ports`, `pid`) is written back without
/// it - the refused key never round-trips.
pub fn to_toml_string(games: &[UserGameProfile], servers: &[UserServer]) -> String {
    let mut out = String::new();
    out.push_str("# LightSpeed user profiles (schema v1) - hand-editable.\n");
    out.push_str("version = 1\n");
    for g in games {
        out.push_str("\n[[games]]\n");
        out.push_str(&format!("name = {}\n", toml_string(&g.name)));
        out.push_str(&format!("key = {}\n", toml_string(&g.key)));
        if !g.process_names.is_empty() {
            let names: Vec<String> = g.process_names.iter().map(|n| toml_string(n)).collect();
            out.push_str(&format!("process_names = [{}]\n", names.join(", ")));
        }
        if let Some(p) = g.udp_ports {
            out.push_str(&format!("udp_ports = {}\n", toml_string(&p.to_string())));
        }
        if g.dynamic_server {
            out.push_str("dynamic_server = true\n");
        }
        if let Some(notes) = &g.notes {
            if !notes.is_empty() {
                out.push_str(&format!("notes = {}\n", toml_string(notes)));
            }
        }
    }
    for s in servers {
        out.push_str("\n[[servers]]\n");
        out.push_str(&format!("name = {}\n", toml_string(&s.name)));
        out.push_str(&format!("address = {}\n", toml_string(&s.address)));
        if let Some(p) = s.udp_ports {
            out.push_str(&format!("udp_ports = {}\n", toml_string(&p.to_string())));
        }
    }
    out
}

// ── entry parsing ────────────────────────────────────────────────────

fn parse_game_entry(entry: &str, value: &toml::Value) -> Result<UserGameProfile, Vec<Diagnostic>> {
    let table = match value.as_table() {
        Some(t) => t,
        None => {
            return Err(vec![Diagnostic::skipped(
                entry,
                format!(
                    "a [[games]] entry must be a table, got {}",
                    value.type_str()
                ),
            )])
        }
    };
    if let Some(d) = rejected_key(table, entry) {
        return Err(vec![d]);
    }

    let mut problems: Vec<String> = Vec::new();

    let name = match take_str(table, "name") {
        Ok(Some(n)) if !n.trim().is_empty() => n.trim().to_string(),
        Ok(_) => {
            problems.push("'name' is required".to_string());
            String::new()
        }
        Err(e) => {
            problems.push(e);
            String::new()
        }
    };

    let key = match take_str(table, "key") {
        Ok(Some(k)) => {
            let k = k.trim().to_string();
            if !is_valid_key(&k) {
                problems.push(format!("invalid key '{k}': must match [a-z0-9-]{{1,24}}"));
            }
            k
        }
        Ok(None) => match slugify(&name) {
            Some(k) => k,
            None => {
                problems.push(format!(
                    "cannot derive a key from name '{name}'; set 'key' explicitly"
                ));
                String::new()
            }
        },
        Err(e) => {
            problems.push(e);
            String::new()
        }
    };

    let process_names = match take_str_array(table, "process_names") {
        Ok(v) => v.unwrap_or_default(),
        Err(e) => {
            problems.push(e);
            Vec::new()
        }
    };
    for p in &process_names {
        if p.trim().is_empty() {
            problems.push("process_names contains an empty name".to_string());
        }
    }

    let udp_ports = match take_ports(table, "udp_ports") {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            None
        }
    };

    let dynamic_server = match take_bool(table, "dynamic_server") {
        Ok(v) => v.unwrap_or(false),
        Err(e) => {
            problems.push(e);
            false
        }
    };

    let notes = match take_str(table, "notes") {
        Ok(Some(n)) if !n.trim().is_empty() => Some(n),
        Ok(_) => None,
        Err(e) => {
            problems.push(e);
            None
        }
    };

    if process_names.is_empty() && udp_ports.is_none() {
        problems.push(format!(
            "game '{name}' defines neither process_names nor udp_ports"
        ));
    }

    if !problems.is_empty() {
        return Err(vec![Diagnostic::skipped(entry, problems.join("; "))]);
    }

    Ok(UserGameProfile {
        name,
        key,
        process_names,
        udp_ports,
        dynamic_server,
        notes,
    })
}

fn parse_server_entry(entry: &str, value: &toml::Value) -> Result<UserServer, Vec<Diagnostic>> {
    let table = match value.as_table() {
        Some(t) => t,
        None => {
            return Err(vec![Diagnostic::skipped(
                entry,
                format!(
                    "a [[servers]] entry must be a table, got {}",
                    value.type_str()
                ),
            )])
        }
    };
    if let Some(d) = rejected_key(table, entry) {
        return Err(vec![d]);
    }

    let mut problems: Vec<String> = Vec::new();

    let name = match take_str(table, "name") {
        Ok(Some(n)) if !n.trim().is_empty() => n.trim().to_string(),
        Ok(_) => {
            problems.push("'name' is required".to_string());
            String::new()
        }
        Err(e) => {
            problems.push(e);
            String::new()
        }
    };

    let address = match take_str(table, "address") {
        Ok(Some(a)) => {
            let a = a.trim().to_string();
            if let Err(reason) = validate_address(&a) {
                problems.push(reason);
            }
            a
        }
        Ok(None) => {
            problems.push("'address' is required".to_string());
            String::new()
        }
        Err(e) => {
            problems.push(e);
            String::new()
        }
    };

    let udp_ports = match take_ports(table, "udp_ports") {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            None
        }
    };

    if !problems.is_empty() {
        return Err(vec![Diagnostic::skipped(entry, problems.join("; "))]);
    }

    Ok(UserServer {
        name,
        address,
        udp_ports,
    })
}

/// The first rejected key found on a raw entry, with its reason.
fn rejected_key(table: &toml::Table, entry: &str) -> Option<Diagnostic> {
    REJECTED_KEYS.iter().find_map(|(key, reason)| {
        table
            .get(*key)
            .map(|_| Diagnostic::rejected(entry, format!("'{key}' is rejected: {reason}")))
    })
}

fn validate_address(address: &str) -> Result<SocketAddrV4, String> {
    address.parse::<SocketAddrV4>().map_err(|_| {
        format!("address '{address}' is not ip:port (expected e.g. 203.0.113.7:27015)")
    })
}

// ── shared checks (load warning == save error) ───────────────────────

/// Collisions with built-ins, and duplicate user keys. Warning severity: the
/// loader keeps the entry (a built-in outranks it at resolve time) and reports
/// it; the saver turns the same message into a hard error.
fn document_diagnostics(games: &[UserGameProfile]) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (i, g) in games.iter().enumerate() {
        if let Some(first) = seen.insert(g.key.as_str(), i) {
            out.push(Diagnostic::warning(
                format!("games[{i}]"),
                format!(
                    "duplicate profile key '{}', also defined by games[{first}]",
                    g.key
                ),
            ));
        }
    }

    for (i, g) in games.iter().enumerate() {
        if let Some(builtin) = builtin_owner_of_key(&g.key) {
            out.push(Diagnostic::warning(
                format!("games[{i}]"),
                format!("key '{}' is reserved by built-in game '{builtin}'", g.key),
            ));
        }
        for process in &g.process_names {
            if let Some(builtin) = builtin_owner_of_process_name(process) {
                out.push(Diagnostic::warning(
                    format!("games[{i}]"),
                    format!("process name '{process}' is reserved by built-in game '{builtin}'"),
                ));
            }
        }
    }

    out
}

/// The display name of the built-in game that claims `key`, if any.
///
/// Probing `detect_game` is deliberate: its match arms *are* the built-in key
/// and alias set (`r6`, `mc`, `pz`, ...), so the collision rule cannot drift
/// from the resolver that decides precedence.
fn builtin_owner_of_key(key: &str) -> Option<String> {
    crate::games::detect_game(key)
        .ok()
        .map(|game| game.name().to_string())
}

/// The display name of the built-in game that claims `process`, if any.
///
/// Image names compare case-insensitively, as they do on every platform the
/// scanner runs on.
fn builtin_owner_of_process_name(process: &str) -> Option<String> {
    for (_, display) in crate::games::all_game_keys() {
        if crate::games::process_names_for_name(display)
            .iter()
            .any(|known| known.eq_ignore_ascii_case(process))
        {
            return Some(display.to_string());
        }
    }
    None
}

/// Strict re-validation of typed values, so the saver enforces the schema on a
/// document that was never parsed from text (a GUI-constructed one).
fn typed_errors(games: &[UserGameProfile], servers: &[UserServer]) -> Vec<SaveError> {
    let mut errors = Vec::new();

    if games.len() > MAX_GAMES {
        errors.push(SaveError::new(
            FILE,
            format!("too many games: {} (the cap is {MAX_GAMES})", games.len()),
        ));
    }
    if servers.len() > MAX_SERVERS {
        errors.push(SaveError::new(
            FILE,
            format!(
                "too many servers: {} (the cap is {MAX_SERVERS})",
                servers.len()
            ),
        ));
    }

    for (i, g) in games.iter().enumerate() {
        let label = format!("games[{i}]");
        if g.name.trim().is_empty() {
            errors.push(SaveError::new(label.as_str(), "'name' is required"));
        }
        if !is_valid_key(&g.key) {
            errors.push(SaveError::new(
                label.as_str(),
                format!("invalid key '{}': must match [a-z0-9-]{{1,24}}", g.key),
            ));
        }
        if g.process_names.iter().any(|p| p.trim().is_empty()) {
            errors.push(SaveError::new(
                label.as_str(),
                "process_names contains an empty name",
            ));
        }
        if g.process_names.is_empty() && g.udp_ports.is_none() {
            errors.push(SaveError::new(
                label.as_str(),
                format!(
                    "game '{}' defines neither process_names nor udp_ports",
                    g.name
                ),
            ));
        }
    }

    for (i, s) in servers.iter().enumerate() {
        let label = format!("servers[{i}]");
        if s.name.trim().is_empty() {
            errors.push(SaveError::new(label.as_str(), "'name' is required"));
        }
        if let Err(reason) = validate_address(&s.address) {
            errors.push(SaveError::new(label.as_str(), reason));
        }
    }

    errors.extend(
        document_diagnostics(games)
            .into_iter()
            .map(|d| SaveError::new(d.entry, d.message)),
    );
    errors
}

fn dedup(errors: Vec<SaveError>) -> Vec<SaveError> {
    let mut seen = HashSet::new();
    errors
        .into_iter()
        .filter(|e| seen.insert((e.entry.clone(), e.message.clone())))
        .collect()
}

// ── raw TOML access ─────────────────────────────────────────────────

fn take_entry_array(
    table: &toml::Table,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<toml::Value> {
    match table.get(key) {
        None => Vec::new(),
        Some(toml::Value::Array(items)) => items.clone(),
        Some(other) => {
            diagnostics.push(Diagnostic::skipped(
                FILE,
                format!(
                    "'{key}' must be an array of tables, got {}",
                    other.type_str()
                ),
            ));
            Vec::new()
        }
    }
}

fn take_str(table: &toml::Table, key: &str) -> Result<Option<String>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            "'{key}' must be a string, got {}",
            other.type_str()
        )),
    }
}

fn take_bool(table: &toml::Table, key: &str) -> Result<Option<bool>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Boolean(b)) => Ok(Some(*b)),
        Some(other) => Err(format!(
            "'{key}' must be true or false, got {}",
            other.type_str()
        )),
    }
}

fn take_i64(table: &toml::Table, key: &str) -> Result<Option<i64>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(v)) => Ok(Some(*v)),
        Some(other) => Err(format!(
            "'{key}' must be an integer, got {}",
            other.type_str()
        )),
    }
}

/// A list of strings. A single string is accepted as a one-element list
/// (tolerant-but-visible: `process_names = "MyGame.exe"` is a common slip and
/// silently reading it as "no names" would be worse).
fn take_str_array(table: &toml::Table, key: &str) -> Result<Option<Vec<String>>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(vec![s.trim().to_string()])),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(s) => out.push(s.trim().to_string()),
                    other => {
                        return Err(format!(
                            "'{key}' must be a list of strings, got a {} in the list",
                            other.type_str()
                        ))
                    }
                }
            }
            Ok(Some(out))
        }
        Some(other) => Err(format!(
            "'{key}' must be a list of strings, got {}",
            other.type_str()
        )),
    }
}

/// `udp_ports`: `"27015-27050"`, `"27015"`, an integer single port, or blank.
fn take_ports(table: &toml::Table, key: &str) -> Result<Option<PortRange>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => parse_port_range(s).map_err(|e| format!("'{key}' {e}")),
        Some(toml::Value::Integer(p)) => {
            let p = u16::try_from(*p)
                .map_err(|_| format!("'{key}' {p} is not a port (the range is 1-65535)"))?;
            PortRange::new(p, p)
                .map(Some)
                .map_err(|e| format!("'{key}': {e}"))
        }
        Some(other) => Err(format!(
            "'{key}' must be a string like \"27015-27050\", got {}",
            other.type_str()
        )),
    }
}

/// A basic TOML string: escape what a basic string cannot carry verbatim.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
version = 1

[[games]]
name = "My Game"
key = "my-game"
process_names = ["MyGame.exe"]
udp_ports = "27015-27050"
dynamic_server = true
notes = "Community server uses UDP 27015"

[[servers]]
name = "My community server"
address = "203.0.113.7:27015"
udp_ports = "27015-27050"
"#;

    fn only_game(text: &str) -> LoadedProfiles {
        let doc = load_from_str(text);
        assert_eq!(
            doc.games.len(),
            1,
            "expected one game, diagnostics: {:?}",
            doc.diagnostics
        );
        doc
    }

    // ── round trip ──────────────────────────────────────────────────

    #[test]
    fn round_trip_preserves_games_and_servers() {
        let doc = load_from_str(SAMPLE);
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
        assert_eq!(doc.version, Some(1));

        let written = to_toml_string(&doc.games, &doc.servers);
        let back = load_from_str(&written);
        assert!(back.diagnostics.is_empty(), "{:?}", back.diagnostics);
        assert_eq!(back.games, doc.games);
        assert_eq!(back.servers, doc.servers);
        assert_eq!(back, doc);
    }

    #[test]
    fn round_trip_is_stable_across_a_second_pass() {
        let doc = load_from_str(SAMPLE);
        let once = to_toml_string(&doc.games, &doc.servers);
        let twice = to_toml_string(&load_from_str(&once).games, &load_from_str(&once).servers);
        assert_eq!(once, twice);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let doc = only_game(
            r#"
[[games]]
name = "My Game"
udp_ports = "27015"
future_field = "whatever"
"#,
        );
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
        assert_eq!(doc.games[0].key, "my-game");
        assert_eq!(doc.games[0].udp_ports, PortRange::new(27015, 27015).ok());
    }

    // ── rejected keys ───────────────────────────────────────────────

    #[test]
    fn tcp_ports_is_rejected_with_a_reason_at_load() {
        let doc = load_from_str(
            r#"
[[games]]
name = "My Game"
udp_ports = "27015"
tcp_ports = "25565"
"#,
        );
        assert!(doc.games.is_empty(), "a tcp_ports entry must not load");
        assert_eq!(doc.diagnostics.len(), 1, "{:?}", doc.diagnostics);
        let d = &doc.diagnostics[0];
        assert_eq!(d.severity, Severity::Rejected);
        assert_eq!(d.entry, "games[0]");
        assert!(d.message.contains("tcp_ports"), "{}", d.message);
        assert!(d.message.contains("UDP-only"), "{}", d.message);
    }

    #[test]
    fn pid_is_rejected_with_a_reason_at_load() {
        let doc = load_from_str(
            r#"
[[games]]
name = "My Game"
process_names = ["MyGame.exe"]
pid = 1234
"#,
        );
        assert!(doc.games.is_empty());
        assert_eq!(doc.diagnostics[0].severity, Severity::Rejected);
        assert!(
            doc.diagnostics[0].message.contains("pid"),
            "{:?}",
            doc.diagnostics
        );
    }

    #[test]
    fn rejected_keys_are_hard_errors_at_save_and_never_round_trip() {
        let text = r#"
[[games]]
name = "My Game"
udp_ports = "27015"
tcp_ports = "25565"
"#;
        let errors = validate_text_for_save(text).expect_err("save must refuse tcp_ports");
        assert!(
            errors.iter().any(|e| e.message.contains("tcp_ports")),
            "{errors:?}"
        );

        // A load that skipped the entry writes a document without the dead key.
        let doc = load_from_str(text);
        let written = to_toml_string(&doc.games, &doc.servers);
        assert!(!written.contains("tcp_ports"), "{written}");
        assert!(!written.contains("pid"), "{written}");
    }

    #[test]
    fn a_valid_document_saves() {
        let doc = load_from_str(SAMPLE);
        assert!(validate_for_save(&doc.games, &doc.servers).is_ok());
        assert!(validate_text_for_save(SAMPLE).is_ok());
    }

    // ── keys ────────────────────────────────────────────────────────

    #[test]
    fn bad_key_is_skipped_and_reported() {
        let doc = load_from_str(
            r#"
[[games]]
name = "My Game"
key = "My Game!"
udp_ports = "27015"
"#,
        );
        assert!(doc.games.is_empty());
        assert_eq!(doc.diagnostics[0].severity, Severity::Skipped);
        assert!(
            doc.diagnostics[0].message.contains("invalid key"),
            "{:?}",
            doc.diagnostics
        );

        let errors = validate_text_for_save(
            "[[games]]\nname = \"My Game\"\nkey = \"My Game!\"\nudp_ports = \"27015\"\n",
        )
        .expect_err("save must refuse an invalid key");
        assert!(
            errors.iter().any(|e| e.message.contains("invalid key")),
            "{errors:?}"
        );
    }

    #[test]
    fn key_is_slugified_from_the_name() {
        let doc = only_game("[[games]]\nname = \"My Game 2!\"\nudp_ports = \"27015\"\n");
        assert_eq!(doc.games[0].key, "my-game-2");

        let mut long = String::from("[[games]]\nname = \"");
        long.push_str(&"a".repeat(60));
        long.push_str("\"\nudp_ports = \"27015\"\n");
        let doc = only_game(&long);
        assert_eq!(doc.games[0].key.len(), KEY_MAX_LEN);
        assert!(is_valid_key(&doc.games[0].key));
    }

    #[test]
    fn a_name_with_no_ascii_alphanumerics_needs_an_explicit_key() {
        let doc = load_from_str("[[games]]\nname = \"ゲーム\"\nudp_ports = \"27015\"\n");
        assert!(doc.games.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("cannot derive a key"),
            "{:?}",
            doc.diagnostics
        );

        let doc =
            only_game("[[games]]\nname = \"ゲーム\"\nkey = \"my-game\"\nudp_ports = \"27015\"\n");
        assert_eq!(doc.games[0].key, "my-game");
    }

    #[test]
    fn duplicate_user_key_warns_at_load_and_errors_at_save() {
        let text = r#"
[[games]]
name = "One"
key = "same"
udp_ports = "27015"

[[games]]
name = "Two"
key = "same"
udp_ports = "27016"
"#;
        let doc = load_from_str(text);
        assert_eq!(
            doc.games.len(),
            2,
            "a duplicate key is a warning, not a skip"
        );
        let w = doc
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Warning)
            .expect("expected a duplicate-key warning");
        assert!(
            w.message.contains("duplicate profile key 'same'"),
            "{}",
            w.message
        );
        assert!(w.message.contains("games[0]"), "{}", w.message);

        let errors = validate_text_for_save(text).expect_err("save must refuse duplicate keys");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("duplicate profile key 'same'")),
            "{errors:?}"
        );
    }

    #[test]
    fn builtin_key_collision_warns_at_load_and_errors_at_save() {
        let text = "[[games]]\nname = \"Sneaky\"\nkey = \"minecraft\"\nudp_ports = \"27015\"\n";
        let doc = load_from_str(text);
        assert_eq!(doc.games.len(), 1);
        let w = doc
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Warning)
            .unwrap();
        assert_eq!(
            w.message,
            "key 'minecraft' is reserved by built-in game 'Minecraft'"
        );

        let errors = validate_text_for_save(text).expect_err("save must refuse a built-in key");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("reserved by built-in game 'Minecraft'")),
            "{errors:?}"
        );
    }

    #[test]
    fn builtin_alias_collision_warns_at_load_and_errors_at_save() {
        for (alias, builtin) in [
            ("r6", "Rainbow Six Siege"),
            ("mc", "Minecraft"),
            ("pz", "Project Zomboid"),
        ] {
            let text =
                format!("[[games]]\nname = \"Sneaky\"\nkey = \"{alias}\"\nudp_ports = \"27015\"\n");
            let doc = load_from_str(&text);
            let w = doc
                .diagnostics
                .iter()
                .find(|d| d.severity == Severity::Warning)
                .unwrap_or_else(|| panic!("no warning for alias {alias}: {:?}", doc.diagnostics));
            assert_eq!(
                w.message,
                format!("key '{alias}' is reserved by built-in game '{builtin}'")
            );

            let errors = validate_text_for_save(&text).expect_err("save must refuse an alias");
            assert!(
                errors.iter().any(|e| e
                    .message
                    .contains(&format!("reserved by built-in game '{builtin}'"))),
                "{errors:?}"
            );
        }
    }

    #[test]
    fn builtin_process_name_collision_warns_at_load_and_errors_at_save() {
        let text = "[[games]]\nname = \"Sneaky\"\nprocess_names = [\"cs2.exe\"]\n";
        let doc = load_from_str(text);
        assert_eq!(doc.games.len(), 1);
        let w = doc
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Warning)
            .unwrap();
        assert_eq!(
            w.message,
            "process name 'cs2.exe' is reserved by built-in game 'Counter-Strike 2'"
        );

        let errors = validate_text_for_save(text).expect_err("save must refuse a built-in name");
        assert!(
            errors.iter().any(|e| e.message
                == "process name 'cs2.exe' is reserved by built-in game 'Counter-Strike 2'"),
            "{errors:?}"
        );

        // Case-insensitive, as the scanner is.
        let doc = load_from_str("[[games]]\nname = \"Sneaky\"\nprocess_names = [\"CS2.EXE\"]\n");
        assert!(doc.diagnostics.iter().any(|d| d
            .message
            .contains("reserved by built-in game 'Counter-Strike 2'")));
    }

    #[test]
    fn a_users_own_ports_and_names_are_not_reserved() {
        let doc = only_game(
            "[[games]]\nname = \"My Game\"\nprocess_names = [\"MyGame.exe\", \"MyGameLauncher.exe\"]\nudp_ports = \"28015\"\n",
        );
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    }

    // ── empty / partial entries ─────────────────────────────────────

    #[test]
    fn empty_game_is_skipped_and_reported() {
        let doc = load_from_str("[[games]]\nname = \"Nothing\"\n");
        assert!(doc.games.is_empty());
        assert_eq!(doc.diagnostics[0].severity, Severity::Skipped);
        assert!(
            doc.diagnostics[0]
                .message
                .contains("defines neither process_names nor udp_ports"),
            "{:?}",
            doc.diagnostics
        );

        let errors = validate_text_for_save("[[games]]\nname = \"Nothing\"\n")
            .expect_err("save must refuse an empty game");
        assert!(errors
            .iter()
            .any(|e| e.message.contains("neither process_names nor udp_ports")));
    }

    #[test]
    fn a_game_with_only_process_names_loads() {
        let doc = only_game("[[games]]\nname = \"My Game\"\nprocess_names = [\"MyGame.exe\"]\n");
        assert_eq!(doc.games[0].udp_ports, None);
    }

    #[test]
    fn blank_udp_ports_means_no_range() {
        let doc = load_from_str("[[games]]\nname = \"My Game\"\nudp_ports = \"\"\n");
        assert!(
            doc.games.is_empty(),
            "blank ports do not satisfy the at-least-one rule"
        );
    }

    // ── port ranges ─────────────────────────────────────────────────

    #[test]
    fn port_ranges_reuse_the_gui_semantics() {
        assert_eq!(
            parse_port_range("  27015-27050  "),
            Ok(Some(PortRange {
                lo: 27015,
                hi: 27050
            }))
        );
        assert_eq!(
            parse_port_range("27015"),
            Ok(Some(PortRange {
                lo: 27015,
                hi: 27015
            }))
        );
        assert_eq!(parse_port_range(""), Ok(None));
        assert_eq!(parse_port_range("   "), Ok(None));
        assert_eq!(
            PortRange {
                lo: 27015,
                hi: 27015
            }
            .to_string(),
            "27015"
        );
        assert_eq!(
            PortRange {
                lo: 27015,
                hi: 27050
            }
            .to_string(),
            "27015-27050"
        );
    }

    #[test]
    fn bad_ranges_are_skipped_and_reported() {
        for (value, needle) in [
            ("\"30500-27000\"", "lo > hi"),
            ("\"0-65535\"", "port 0 is not a valid port"),
            ("\"70000\"", "is not a port range"),
            ("\"abc\"", "is not a port range"),
            ("\"1.5\"", "is not a port range"),
        ] {
            let text = format!("[[games]]\nname = \"My Game\"\nudp_ports = {value}\n");
            let doc = load_from_str(&text);
            assert!(doc.games.is_empty(), "{value} must be skipped");
            let msg = &doc.diagnostics[0].message;
            assert!(
                msg.contains(needle),
                "{value}: expected {needle:?} in {msg:?}"
            );

            let errors = match validate_text_for_save(&text) {
                Ok(_) => panic!("{value} must be refused at save with {needle}"),
                Err(errors) => errors,
            };
            assert!(
                errors.iter().any(|e| e.message.contains(needle)),
                "{errors:?}"
            );
        }
    }

    #[test]
    fn an_integer_udp_port_is_a_single_port_range() {
        let doc = only_game("[[games]]\nname = \"My Game\"\nudp_ports = 27015\n");
        assert_eq!(
            doc.games[0].udp_ports,
            Some(PortRange {
                lo: 27015,
                hi: 27015
            })
        );
    }

    #[test]
    fn a_wrong_typed_udp_ports_is_skipped_and_reported() {
        let doc = load_from_str("[[games]]\nname = \"My Game\"\nudp_ports = [27015]\n");
        assert!(doc.games.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("udp_ports"),
            "{:?}",
            doc.diagnostics
        );
    }

    // ── caps ────────────────────────────────────────────────────────

    #[test]
    fn caps_are_enforced_and_reported() {
        let mut text = String::new();
        for i in 0..(MAX_GAMES + 3) {
            text.push_str(&format!(
                "[[games]]\nname = \"Game {i}\"\nkey = \"game-{i}\"\nudp_ports = \"27015\"\n\n"
            ));
        }
        for i in 0..(MAX_SERVERS + 2) {
            text.push_str(&format!(
                "[[servers]]\nname = \"Server {i}\"\naddress = \"203.0.113.{i}:27015\"\n\n"
            ));
        }
        let doc = load_from_str(&text);
        assert_eq!(doc.games.len(), MAX_GAMES);
        assert_eq!(doc.servers.len(), MAX_SERVERS);
        let over = doc
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("-game cap"))
            .count();
        assert_eq!(over, 3, "{:?}", doc.diagnostics);
        let over_servers = doc
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("-server cap"))
            .count();
        assert_eq!(over_servers, 2);
        assert!(doc
            .diagnostics
            .iter()
            .any(|d| d.entry == "games[64]" && d.message.contains("64-game cap")));

        // The cap is a hard error on the save path, where 65 typed games are
        // what the caller wants written.
        let too_many: Vec<UserGameProfile> = (0..(MAX_GAMES + 1))
            .map(|i| UserGameProfile {
                name: format!("Game {i}"),
                key: format!("game-{i}"),
                process_names: vec![],
                udp_ports: PortRange::new(27015, 27015).ok(),
                dynamic_server: false,
                notes: None,
            })
            .collect();
        let errors = validate_for_save(&too_many, &[]).unwrap_err();
        assert!(
            errors.iter().any(|e| e.message
                == format!("too many games: {} (the cap is {MAX_GAMES})", MAX_GAMES + 1)),
            "{errors:?}"
        );
    }

    // ── malformed input ─────────────────────────────────────────────

    #[test]
    fn malformed_entries_are_skipped_and_reported() {
        // A non-table entry inside the array is skipped, the good one survives.
        let doc = load_from_str("games = [{ name = \"Good\", udp_ports = \"27015\" }, 42]\n");
        assert_eq!(doc.games.len(), 1);
        assert_eq!(doc.games[0].name, "Good");
        assert_eq!(doc.diagnostics.len(), 1);
        assert_eq!(doc.diagnostics[0].severity, Severity::Skipped);
        assert_eq!(doc.diagnostics[0].entry, "games[1]");
        assert!(
            doc.diagnostics[0].message.contains("must be a table"),
            "{:?}",
            doc.diagnostics
        );

        // `games` as a scalar: reported, and no games are read from it.
        let doc = load_from_str("games = 3\n");
        assert!(doc.games.is_empty());
        assert!(doc.diagnostics[0].message.contains("array of tables"));

        // Wrong types inside an entry: skipped, and the reason names the field.
        let doc = load_from_str("[[games]]\nname = 42\nudp_ports = \"27015\"\n");
        assert!(doc.games.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("'name'"),
            "{:?}",
            doc.diagnostics
        );

        let doc = load_from_str(
            "[[games]]\nname = \"G\"\nudp_ports = \"27015\"\ndynamic_server = \"yes\"\n",
        );
        assert!(doc.games.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("dynamic_server"),
            "{:?}",
            doc.diagnostics
        );

        let doc = load_from_str("[[games]]\nname = \"G\"\nprocess_names = [1, 2]\n");
        assert!(doc.games.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("list of strings"),
            "{:?}",
            doc.diagnostics
        );
    }

    #[test]
    fn a_broken_document_is_reported_not_panicked() {
        let doc = load_from_str("this is not = = toml");
        assert!(doc.games.is_empty() && doc.servers.is_empty());
        assert_eq!(doc.diagnostics[0].severity, Severity::Skipped);
        assert!(doc.diagnostics[0].message.contains("not valid TOML"));
        assert!(validate_text_for_save("this is not = = toml").is_err());
    }

    #[test]
    fn an_empty_file_is_an_empty_document() {
        let doc = load_from_str("");
        assert!(doc.is_empty());
        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn a_single_string_process_names_is_read_as_one_name() {
        let doc = only_game("[[games]]\nname = \"My Game\"\nprocess_names = \"MyGame.exe\"\n");
        assert_eq!(doc.games[0].process_names, vec!["MyGame.exe".to_string()]);
    }

    // ── servers ─────────────────────────────────────────────────────

    #[test]
    fn servers_need_a_name_and_an_ip_port_address() {
        let doc = load_from_str("[[servers]]\nname = \"srv\"\naddress = \"example.com:27015\"\n");
        assert!(doc.servers.is_empty());
        assert!(
            doc.diagnostics[0].message.contains("is not ip:port"),
            "{:?}",
            doc.diagnostics
        );

        let doc = load_from_str("[[servers]]\nname = \"srv\"\n");
        assert!(doc.servers.is_empty());
        assert!(doc.diagnostics[0].message.contains("'address' is required"));

        let doc = load_from_str("[[servers]]\naddress = \"203.0.113.7:27015\"\n");
        assert!(doc.servers.is_empty());
        assert!(doc.diagnostics[0].message.contains("'name' is required"));

        let errors = validate_text_for_save("[[servers]]\nname = \"srv\"\naddress = \"nope\"\n")
            .expect_err("save must refuse a bad address");
        assert!(errors.iter().any(|e| e.message.contains("is not ip:port")));
    }

    #[test]
    fn a_server_may_carry_its_own_udp_range() {
        let doc = load_from_str(
            "[[servers]]\nname = \"srv\"\naddress = \"203.0.113.7:27015\"\nudp_ports = \"27015-27020\"\n",
        );
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
        assert_eq!(
            doc.servers[0].udp_ports,
            Some(PortRange {
                lo: 27015,
                hi: 27020
            })
        );
        assert_eq!(doc.servers[0].address, "203.0.113.7:27015");
    }

    #[test]
    fn transport_is_always_udp_so_servers_refuse_tcp_ports_too() {
        let doc = load_from_str(
            "[[servers]]\nname = \"srv\"\naddress = \"203.0.113.7:27015\"\ntcp_ports = \"1-2\"\n",
        );
        assert!(doc.servers.is_empty());
        assert_eq!(doc.diagnostics[0].severity, Severity::Rejected);
    }

    #[test]
    fn report_does_not_panic() {
        let doc = load_from_str("[[games]]\nname = \"Nothing\"\n");
        doc.report();
    }
}
