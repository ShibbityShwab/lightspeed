//! Cross-platform "list every running process" enumeration.
//!
//! This is the data source behind a live process picker: instead of asking the
//! user to type an executable name, the GUI can show the machine's process
//! table and let them point at the process they care about.
//!
//! The scanner is deliberately dependency-free (no `sysinfo`, no `windows-sys`),
//! so it adds nothing to the binary size or the supply chain. Windows talks to
//! `kernel32` through locally declared `extern "system"` blocks (the same
//! pattern `client-gui` uses for its `advapi32` token query); Linux reads
//! `/proc`; macOS parses a single `ps` call.
//!
//! Nothing here may abort an enumeration: a process that exits mid-scan, or one
//! that denies the access needed to learn its image path, keeps its row with
//! `None` fields rather than disappearing (or panicking).
//!
//! ## Platform strategies
//!
//! | Platform | Names / PIDs | Path | Start time |
//! |----------|--------------|------|------------|
//! | Windows  | `CreateToolhelp32Snapshot` + `Process32FirstW`/`Process32NextW` | `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW` | `GetProcessTimes` |
//! | Linux    | `/proc/<pid>/comm` | first NUL field of `/proc/<pid>/cmdline` | `starttime` from `/proc/<pid>/stat` + `/proc/uptime` |
//! | macOS    | `ps -axo pid=,comm=,lstart=` | the `comm` column | the `lstart` column |

/// One running process.
///
/// `path` and `started_at` are `None` whenever the OS refuses or fails the
/// lookup (a protected process, a kernel thread, a race with its own exit).
/// The row is still returned: a picker that silently hides half the machine is
/// worse than one that shows an incomplete row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    /// Operating-system process id.
    pub pid: u32,
    /// Image name (`javaw.exe` on Windows, `java` on Linux and macOS).
    pub name: String,
    /// Absolute path of the executable, when the OS will tell us.
    pub path: Option<String>,
    /// Local start time, formatted `YYYY-MM-DD HH:MM:SS`, when knowable.
    pub started_at: Option<String>,
}

/// Enumerate every running process, sorted by name and then by PID.
///
/// Duplicate names are preserved: two `javaw.exe` rows are two different
/// programs, and hiding one would defeat the point of the picker. Returns an
/// empty vector when the platform enumeration is unavailable; it never panics.
pub fn list_running_processes() -> Vec<ProcessEntry> {
    let mut entries = platform_entries();
    entries.sort_by(sort_key);
    entries
}

/// Ordering used by [`list_running_processes`]: image name, then PID.
fn sort_key(a: &ProcessEntry, b: &ProcessEntry) -> std::cmp::Ordering {
    a.name.cmp(&b.name).then(a.pid.cmp(&b.pid))
}

/// Dispatch to the platform enumerator. Every branch fails open.
fn platform_entries() -> Vec<ProcessEntry> {
    #[cfg(target_os = "windows")]
    return windows::list();

    #[cfg(target_os = "linux")]
    return linux::list();

    #[cfg(target_os = "macos")]
    return macos::list();

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Vec::new()
}

/// Format Unix seconds as `YYYY-MM-DD HH:MM:SS` in UTC.
///
/// `None` for a timestamp chrono cannot represent, so a nonsense start time
/// degrades to an absent field instead of a panic.
fn format_unix_timestamp(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Pure parse helpers (text in, values out - unit-tested on every platform)
// ─────────────────────────────────────────────────────────────────────────────

/// First whitespace-separated field of `/proc/uptime`: seconds since boot.
fn parse_uptime(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Field 22 of `/proc/<pid>/stat`: `starttime`, in clock ticks since boot.
///
/// The `comm` field between the parentheses may itself contain spaces and
/// parentheses, so the parse restarts after the **last** `)`. The field that
/// follows it is field 3 (`state`), which puts `starttime` at offset 19 of the
/// remainder.
fn parse_stat_starttime(stat: &str) -> Option<u64> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

/// First NUL-separated field of `/proc/<pid>/cmdline` as the image path.
///
/// Kernel threads report an empty `cmdline`, and the buffer is NUL-padded
/// rather than trimmed, so an empty first field means "no path" - never `""`.
fn parse_cmdline_path(cmdline: &[u8]) -> Option<String> {
    let first = cmdline.split(|&b| b == 0).next()?;
    if first.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(first).into_owned())
}

/// Absolute start time from wall clock, `/proc/uptime`, and a tick count.
///
/// `uptime_secs` is seconds since boot, so the boot instant is
/// `now - uptime`; `starttime` ticks are then added on top.
fn start_time_unix(now_secs: i64, uptime_secs: f64, starttime_ticks: u64, hz: u64) -> Option<i64> {
    if hz == 0 || !uptime_secs.is_finite() {
        return None;
    }
    let boot = now_secs - uptime_secs.round() as i64;
    Some(boot + (starttime_ticks / hz) as i64)
}

/// Split macOS `comm` into a short name and, when it is a path, that path.
///
/// `comm` is a full path for most binaries (`/usr/sbin/sshd`) but a bare name
/// for some kernel-bundled ones; the picker wants the short name either way.
fn split_comm(comm: &str) -> (String, Option<String>) {
    match comm.rsplit_once('/') {
        Some((_, base)) if !base.is_empty() => (base.to_string(), Some(comm.to_string())),
        _ => (comm.to_string(), None),
    }
}

/// Parse one `ps -axo pid=,comm=,lstart=` row.
///
/// `lstart` is five space-separated fields (`Wed Oct  1 12:34:56 2025`) and
/// `comm` is a path that may contain spaces, so the row is split from the
/// right: PID first, then `lstart`'s five fields, and everything between is
/// `comm`.
fn parse_ps_line(line: &str) -> Option<ProcessEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    // 1 (pid) + 1 (comm) + 5 (lstart) is the minimum shape.
    if fields.len() < 7 {
        return None;
    }
    let pid: u32 = fields[0].parse().ok()?;
    let started_at = fields[fields.len() - 5..].join(" ");
    let comm = fields[1..fields.len() - 5].join(" ");
    if comm.is_empty() {
        return None;
    }
    let (name, path) = split_comm(&comm);
    Some(ProcessEntry {
        pid,
        name,
        path,
        started_at: Some(started_at),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Windows: Toolhelp32 snapshot + per-PID queries
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use super::{format_unix_timestamp, ProcessEntry};
    use std::ffi::c_void;

    /// `TH32CS_SNAPPROCESS`: include every process in the snapshot.
    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    /// `INVALID_HANDLE_VALUE` (`(HANDLE)-1`), the snapshot call's failure value.
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;
    /// Least privilege that still answers both the path and the time query.
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// `MAX_PATH` for the `szExeFile` member of `PROCESSENTRY32W`.
    const MAX_PATH: usize = 260;
    /// Longest image path Windows returns, including a `\\?\` extended prefix.
    const MAX_IMAGE_PATH: usize = 32_768;
    /// 100 ns ticks between the Windows epoch (1601) and the Unix epoch (1970).
    const EPOCH_DIFF_TICKS: u64 = 116_444_736_000_000_000;

    /// `PROCESSENTRY32W`. Field order and widths are the ABI contract; the
    /// parent PID is retained so the struct matches what the API writes.
    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; MAX_PATH],
    }

    /// `FILETIME`: 100 ns ticks since 1601-01-01, split into two halves.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    /// Walk the snapshot; never panics, and returns an empty list if the
    /// snapshot itself cannot be taken.
    pub(super) fn list() -> Vec<ProcessEntry> {
        // SAFETY: the snapshot handle is checked against INVALID_HANDLE_VALUE
        // and null, and is closed on every path. `raw` is a packed-by-hand
        // PROCESSENTRY32W whose dwSize is set before each call, which is what
        // the API requires; the API writes only within that struct.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
                return Vec::new();
            }
            let mut entries = Vec::new();
            let mut raw: ProcessEntry32W = std::mem::zeroed();
            raw.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
            if Process32FirstW(snapshot, &mut raw) != 0 {
                loop {
                    let pid = raw.th32_process_id;
                    let name = wide_to_string(&raw.sz_exe_file);
                    let (path, started_at) = query_details(pid);
                    entries.push(ProcessEntry {
                        pid,
                        name,
                        path,
                        started_at,
                    });
                    raw.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
                    if Process32NextW(snapshot, &mut raw) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            entries
        }
    }

    /// Image path and start time for `pid`, each `None` when its lookup fails.
    ///
    /// `PROCESS_QUERY_LIMITED_INFORMATION` is the least privilege that answers
    /// both queries and is granted for most processes an unprivileged user did
    /// not start. A failure here (system PIDs, protected processes, a racing
    /// exit) leaves a `None` behind - it never drops the row.
    ///
    /// SAFETY: `handle` must be a live process handle from `OpenProcess`.
    unsafe fn query_details(pid: u32) -> (Option<String>, Option<String>) {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return (None, None);
        }
        let path = query_image_path(handle);
        let started_at = query_start_time(handle);
        CloseHandle(handle);
        (path, started_at)
    }

    /// SAFETY: `handle` must be a live process handle.
    unsafe fn query_image_path(handle: *mut c_void) -> Option<String> {
        let mut buffer = vec![0u16; MAX_IMAGE_PATH];
        let mut len = buffer.len() as u32;
        if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        buffer.truncate(len as usize);
        Some(wide_to_string(&buffer))
    }

    /// SAFETY: `handle` must be a live process handle.
    unsafe fn query_start_time(handle: *mut c_void) -> Option<String> {
        let mut creation = FileTime { low: 0, high: 0 };
        let (mut exit, mut kernel, mut user) = (creation, creation, creation);
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return None;
        }
        let ticks = ((creation.high as u64) << 32) | u64::from(creation.low);
        if ticks < EPOCH_DIFF_TICKS {
            return None;
        }
        format_unix_timestamp(((ticks - EPOCH_DIFF_TICKS) / 10_000_000) as i64)
    }

    /// A NUL-terminated (or full) UTF-16 buffer as a `String`, lossily: the
    /// image name is only ever displayed, never used as a path on disk.
    fn wide_to_string(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn wide_string_stops_at_the_terminator() {
            let buf = [b'a' as u16, b'b' as u16, 0, b'c' as u16];
            assert_eq!(wide_to_string(&buf), "ab");
        }

        #[test]
        fn wide_string_handles_a_full_buffer_without_a_terminator() {
            let buf = [b'x' as u16; 4];
            assert_eq!(wide_to_string(&buf), "xxxx");
        }

        /// The field offsets are the ABI contract: the API validates `dwSize`
        /// against its own `sizeof(PROCESSENTRY32W)` and writes the struct by
        /// these offsets, so a mismatch here is an empty enumeration at runtime.
        #[test]
        fn process_entry32w_layout_matches_the_abi() {
            use std::mem::{offset_of, size_of};
            // 4+4+4 (+4 pad) +8 +4+4+4+4+4 +520, rounded to the pointer alignment.
            assert_eq!(size_of::<ProcessEntry32W>(), 568);
            assert_eq!(offset_of!(ProcessEntry32W, dw_size), 0);
            assert_eq!(offset_of!(ProcessEntry32W, th32_process_id), 8);
            assert_eq!(offset_of!(ProcessEntry32W, sz_exe_file), 44);
        }

        /// Live call against this host's real process table. Ignored by
        /// default so the unit suite stays deterministic; run explicitly with
        /// `cargo test -p lightspeed-client processes:: -- --ignored --nocapture`.
        #[test]
        #[ignore = "enumerates this host's real process table"]
        fn windows_lists_this_hosts_processes() {
            let entries = super::super::list_running_processes();
            println!("{} processes", entries.len());
            for entry in entries.iter().take(10) {
                println!(
                    "{:<28} {:>6}  {:<50} {}",
                    entry.name,
                    entry.pid,
                    entry.path.as_deref().unwrap_or("<no path>"),
                    entry.started_at.as_deref().unwrap_or("<no start time>")
                );
            }
            assert!(!entries.is_empty(), "a live Windows host has processes");
            assert!(
                entries.windows(2).all(|w| w[0].name <= w[1].name),
                "result must be name-sorted"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Linux: /proc walk
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::{
        format_unix_timestamp, parse_cmdline_path, parse_stat_starttime, parse_uptime,
        start_time_unix, ProcessEntry,
    };

    /// Clock ticks per second that `/proc/<pid>/stat` `starttime` is expressed
    /// in (`sysconf(_SC_CLK_TCK)`, 100 on every mainstream Linux).
    const CLK_TCK: u64 = 100;

    /// Walk `/proc/<pid>/` for numeric directories; every read error is a
    /// `continue` or a `None`, so a process exiting mid-scan cannot abort it.
    pub(super) fn list() -> Vec<ProcessEntry> {
        let uptime = read_uptime_secs();
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for entry in dir.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue; // not a numeric PID directory
            };
            let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) else {
                continue;
            };
            let path = std::fs::read(format!("/proc/{pid}/cmdline"))
                .ok()
                .and_then(|raw| parse_cmdline_path(&raw));
            let started_at = uptime
                .and_then(|up| {
                    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                    let ticks = parse_stat_starttime(&stat)?;
                    let now = chrono::Utc::now().timestamp();
                    start_time_unix(now, up, ticks, CLK_TCK)
                })
                .and_then(format_unix_timestamp);
            entries.push(ProcessEntry {
                pid,
                name: comm.trim().to_string(),
                path,
                started_at,
            });
        }
        entries
    }

    fn read_uptime_secs() -> Option<f64> {
        parse_uptime(&std::fs::read_to_string("/proc/uptime").ok()?)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  macOS: one `ps` call
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use super::{parse_ps_line, ProcessEntry};

    /// `ps -axo pid=,comm=,lstart=` - the `=` suffixes suppress the header and
    /// give one line per process. A missing `ps` yields an empty list.
    pub(super) fn list() -> Vec<ProcessEntry> {
        let output = match crate::process::silent_command("ps")
            .args(["-axo", "pid=,comm=,lstart="])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => return Vec::new(),
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_ps_line)
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn parse_uptime_reads_the_first_field() {
        assert_eq!(parse_uptime("12345.67 98765.43\n"), Some(12345.67));
        assert_eq!(parse_uptime(""), None);
        assert_eq!(parse_uptime("not-a-number 1.0"), None);
    }

    /// `stat` field 22 is the 20th whitespace field after the last `)`.
    #[test]
    fn parse_stat_starttime_reads_field_22() {
        let stat =
            "1234 (java) S 1 1234 1234 0 -1 4194304 100 0 0 0 5 6 7 8 20 0 12 0 987654 123 456";
        assert_eq!(parse_stat_starttime(stat), Some(987654));
    }

    /// `comm` may contain both spaces and parentheses; the parse must key off
    /// the last `)`, not the first.
    #[test]
    fn parse_stat_starttime_survives_a_hostile_comm() {
        let stat = "7 (weird ) name) R 1 7 7 0 -1 4194304 1 1 1 1 1 1 1 1 20 0 1 0 4242 0 0";
        assert_eq!(parse_stat_starttime(stat), Some(4242));
    }

    #[test]
    fn parse_stat_starttime_rejects_truncated_input() {
        assert_eq!(parse_stat_starttime("42 (java) S 1 2"), None);
        assert_eq!(parse_stat_starttime("no parens at all"), None);
    }

    #[test]
    fn parse_cmdline_path_takes_the_first_nul_field() {
        assert_eq!(
            parse_cmdline_path(b"/usr/bin/java\0-Xmx2g\0Main\0"),
            Some("/usr/bin/java".to_string())
        );
    }

    #[test]
    fn parse_cmdline_path_is_none_for_kernel_threads_and_nul_padding() {
        assert_eq!(parse_cmdline_path(b""), None);
        assert_eq!(parse_cmdline_path(b"\0\0\0"), None);
    }

    #[test]
    fn start_time_unix_adds_ticks_above_the_boot_instant() {
        // 1000 s of uptime against a fixed wall clock, 250 ticks at 100 Hz = 2 s.
        assert_eq!(
            start_time_unix(1_700_000_000, 1000.0, 250, 100),
            Some(1_699_999_002)
        );
        // Nonsense inputs degrade to None rather than to a wrong timestamp.
        assert_eq!(start_time_unix(1_700_000_000, 1000.0, 250, 0), None);
        assert_eq!(start_time_unix(1_700_000_000, f64::NAN, 250, 100), None);
    }

    #[test]
    fn format_unix_timestamp_is_utc() {
        assert_eq!(
            format_unix_timestamp(0).as_deref(),
            Some("1970-01-01 00:00:00")
        );
        assert_eq!(
            format_unix_timestamp(1_699_999_002).as_deref(),
            Some("2023-11-14 21:56:42")
        );
    }

    #[test]
    fn parse_ps_line_splits_pid_comm_and_lstart() {
        let entry = parse_ps_line("  501 /usr/sbin/sshd  Wed Oct  1 12:34:56 2025").unwrap();
        assert_eq!(entry.pid, 501);
        assert_eq!(entry.name, "sshd");
        assert_eq!(entry.path.as_deref(), Some("/usr/sbin/sshd"));
        assert_eq!(entry.started_at.as_deref(), Some("Wed Oct 1 12:34:56 2025"));
    }

    #[test]
    fn parse_ps_line_keeps_spaces_inside_comm() {
        let line = "  42 /Applications/My App.app/Contents/MacOS/My App Tue Sep 30 08:00:01 2025";
        let entry = parse_ps_line(line).unwrap();
        assert_eq!(entry.pid, 42);
        assert_eq!(entry.name, "My App");
        assert_eq!(
            entry.path.as_deref(),
            Some("/Applications/My App.app/Contents/MacOS/My App")
        );
        assert_eq!(
            entry.started_at.as_deref(),
            Some("Tue Sep 30 08:00:01 2025")
        );
    }

    #[test]
    fn parse_ps_line_rejects_rows_that_are_too_short() {
        assert_eq!(parse_ps_line(""), None);
        assert_eq!(parse_ps_line("501 sshd"), None);
        assert_eq!(
            parse_ps_line("not-a-pid sshd Wed Oct 1 12:34:56 2025"),
            None
        );
    }

    #[test]
    fn split_comm_separates_a_path_from_a_bare_name() {
        assert_eq!(
            split_comm("/usr/sbin/sshd"),
            ("sshd".to_string(), Some("/usr/sbin/sshd".to_string()))
        );
        assert_eq!(split_comm("sshd"), ("sshd".to_string(), None));
    }

    #[test]
    fn sort_is_by_name_then_pid_and_keeps_duplicates() {
        let entry = |pid, name: &str| ProcessEntry {
            pid,
            name: name.to_string(),
            path: None,
            started_at: None,
        };
        let mut entries = [
            entry(300, "javaw.exe"),
            entry(10, "svchost.exe"),
            entry(100, "javaw.exe"),
            entry(200, "explorer.exe"),
        ];
        entries.sort_by(sort_key);
        let observed: Vec<(u32, &str)> = entries.iter().map(|e| (e.pid, e.name.as_str())).collect();
        assert_eq!(
            observed,
            vec![
                (200, "explorer.exe"),
                (100, "javaw.exe"),
                (300, "javaw.exe"),
                (10, "svchost.exe"),
            ]
        );
        assert_eq!(entries.len(), 4, "duplicate names must not be deduped");
    }

    #[test]
    fn sort_key_orders_equal_names_by_pid() {
        let a = ProcessEntry {
            pid: 1,
            name: "x".into(),
            path: None,
            started_at: None,
        };
        let b = ProcessEntry {
            pid: 2,
            name: "x".into(),
            path: None,
            started_at: None,
        };
        assert_eq!(sort_key(&a, &b), Ordering::Less);
        assert_eq!(sort_key(&b, &a), Ordering::Greater);
        assert_eq!(sort_key(&a, &a), Ordering::Equal);
    }
}
