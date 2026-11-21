//! Opt-in autostart writers (cross-platform plan G1.7; spec W2-4 (f)).
//!
//! Opt-in is the product rule: autostart is off by default, launches hidden to
//! the tray when a tray exists, and never starts a second engine (the
//! single-instance guard already owns that). This module lands the **writers**
//! as pure builders plus a temp-dir create/remove pair, so the entry format is
//! testable on this host without touching the real registry, `~/.config` or
//! `~/Library`:
//!
//! | OS | Entry | Launch |
//! |---|---|---|
//! | Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `LightSpeed` | `"<exe>" --autostart` |
//! | Linux | `$XDG_CONFIG_HOME/autostart/lightspeed-gui.desktop` | `Exec="<exe>" --autostart` |
//! | macOS | `~/Library/LaunchAgents/dev.lightspeed.gui.plist` | `ProgramArguments = [<exe>, --autostart]` |
//!
//! The Settings toggle is shell work (WF-081b, task-74); this module provides
//! the two calls it makes: [`installed_current`] (the observable truth, read
//! once per Settings open) and [`apply_current`] (write or remove the entry,
//! surfacing a failure instead of claiming success). Nothing here is a
//! behavioural stub: every function is a complete implementation, so the module
//! carries **no** `dead_code` allowance, blanket or local.
//!
//! `--autostart` is the launch mode the entry passes; its contract (argument
//! name, quoting per OS, one entry per install) is unchanged by this wiring.

use std::path::{Path, PathBuf};

/// The launch argument an autostart entry passes.
pub const AUTOSTART_ARG: &str = "--autostart";

/// The registry value name (Windows) and the identity used in logs.
pub const APP_ID: &str = "LightSpeed";

/// The Windows `Run` key both the writer and the existence check address.
const REGISTRY_RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// The three autostart mechanisms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

/// The OS this binary was compiled for.
pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

/// The launch command a login entry runs: the quoted exe plus the autostart
/// argument.
///
/// Quoting follows each mechanism's syntax: a registry `REG_SZ` and a
/// `.desktop` `Exec=` both use double quotes, while the plist takes the raw
/// path as its own array element (XML-escaped).
pub fn launch_command(exe: &Path) -> String {
    format!("\"{}\" {AUTOSTART_ARG}", exe.display())
}

/// A concrete autostart entry: either a file to write or a registry value to
/// create. Pure data, so the format is testable anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutostartPlan {
    /// A file entry (Linux `.desktop`, macOS `.plist`) under `dir`.
    File {
        dir: PathBuf,
        file_name: &'static str,
        contents: String,
    },
    /// The Windows registry value (the real writer is `reg.exe`; the command is
    /// pure data so it can be checked without touching `HKCU`).
    Registry {
        value_name: &'static str,
        command: String,
    },
}

impl AutostartPlan {
    /// A stable kind label for logs and the parity report.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::Registry { .. } => "registry",
        }
    }

    /// The file this plan writes, when it is a file plan.
    pub fn path(&self) -> Option<PathBuf> {
        match self {
            Self::File { dir, file_name, .. } => Some(dir.join(file_name)),
            Self::Registry { .. } => None,
        }
    }

    /// A one-line description for the startup trace (no behaviour).
    pub fn describe(&self) -> String {
        match self {
            Self::File { .. } => match self.path() {
                Some(path) => format!("file {}", path.display()),
                None => "file entry".to_string(),
            },
            Self::Registry { value_name, .. } => format!("registry value {value_name}"),
        }
    }
}

/// Build the autostart entry for `os`.
///
/// `config_dir` is the per-user config root (`~/.config` on Linux, where
/// `dirs::config_dir()` already resolves `XDG_CONFIG_HOME`), `home_dir` the
/// user's home (macOS `Library/LaunchAgents`), and `exe` the installed binary.
pub fn plan(os: Os, exe: &Path, config_dir: &Path, home_dir: &Path) -> AutostartPlan {
    match os {
        Os::Windows => AutostartPlan::Registry {
            value_name: APP_ID,
            command: launch_command(exe),
        },
        Os::Linux => AutostartPlan::File {
            dir: config_dir.join("autostart"),
            file_name: "lightspeed-gui.desktop",
            contents: desktop_entry(exe),
        },
        Os::Macos => AutostartPlan::File {
            dir: home_dir.join("Library").join("LaunchAgents"),
            file_name: "dev.lightspeed.gui.plist",
            contents: launch_agent_plist(exe),
        },
    }
}

/// Plan for this process, when its own path is knowable.
///
/// Used by the startup trace and by the Settings toggle, so the writer half is
/// compiled and observable on every OS.
pub fn plan_for_current() -> Option<AutostartPlan> {
    let exe = std::env::current_exe().ok()?;
    let config_dir = crate::paths::config_dir();
    let home_dir = dirs::home_dir()?;
    Some(plan(current_os(), &exe, &config_dir, &home_dir))
}

/// Whether this install's login entry currently exists.
///
/// This is the observable truth the Settings toggle reconciles against: the
/// Windows `Run` value is queried through `reg.exe`, a Linux/macOS entry is a
/// file under its plan's directory. A plan that cannot be resolved (unknown
/// executable path) cannot be installed, so it reports `false`.
pub fn installed_current() -> bool {
    match plan_for_current() {
        Some(AutostartPlan::File { dir, .. }) => is_installed(&dir, current_os()),
        Some(AutostartPlan::Registry { .. }) => registry_is_installed(),
        None => false,
    }
}

/// Write (`enabled`) or remove the login entry for this install.
///
/// The one path the Settings toggle calls. A failure is surfaced verbatim, so
/// the toggle can report it and put its checkbox back instead of showing a
/// checkmark for an entry that does not exist.
pub fn apply_current(enabled: bool) -> Result<(), String> {
    let plan = plan_for_current().ok_or_else(|| "the executable path is unknown".to_string())?;
    apply_entry(&plan, current_os(), enabled)
}

/// Apply `enabled` to one concrete plan.
///
/// `os` selects the file-entry naming for removal (`exit_file_names`); it is
/// passed explicitly so the temp-dir tests can exercise the Linux/macOS shape
/// on this Windows host. The registry branch is dispatched through
/// [`apply_entry_with`] with the real `reg.exe` runner; tests inject a recorder
/// instead, so a test run never writes the real `HKCU` value.
fn apply_entry(plan: &AutostartPlan, os: Os, enabled: bool) -> Result<(), String> {
    apply_entry_with(plan, os, enabled, &mut run_registry)
}

/// [`apply_entry`] with the registry command runner injected.
///
/// The injection point exists so the enable/disable dispatch can be tested for
/// the Windows shape without touching the real user's login entry.
fn apply_entry_with(
    plan: &AutostartPlan,
    os: Os,
    enabled: bool,
    run_registry_cmd: &mut dyn FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    match plan {
        AutostartPlan::File { dir, .. } => {
            if enabled {
                write_file(dir, plan)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            } else {
                remove_file(dir, os)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
        AutostartPlan::Registry { command, .. } => {
            let args = if enabled {
                registry_add_args(command)
            } else {
                registry_remove_args()
            };
            run_registry_cmd(&args)
        }
    }
}

/// Whether the Windows `Run` value exists.
///
/// `reg.exe` exits non-zero for "not found" and for real errors alike; both mean
/// the entry is not installed, and an execution error is logged rather than
/// hidden.
fn registry_is_installed() -> bool {
    match super::silent_command("reg.exe")
        .args(["query", REGISTRY_RUN_KEY, "/v", APP_ID])
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            tracing::warn!("could not query the autostart registry value: {error}");
            false
        }
    }
}

/// Run the `reg.exe` write/remove command, mapping a failed exit to a message
/// the Settings toggle can show.
fn run_registry(args: &[String]) -> Result<(), String> {
    match super::silent_command("reg.exe").args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("reg.exe {status}")),
        Err(error) => Err(format!("could not run reg.exe: {error}")),
    }
}

/// The Linux `.desktop` entry.
pub fn desktop_entry(exe: &Path) -> String {
    // Desktop Entry spec: quote the Exec argument when it contains spaces and
    // escape backslashes and double quotes inside the quotes.
    let exec = format!(
        "\"{}\" {AUTOSTART_ARG}",
        exe.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={APP_ID}\n\
         Comment=LightSpeed network optimizer\n\
         Exec={exec}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         Hidden=false\n"
    )
}

/// The macOS LaunchAgent plist (an array element, so XML-escaped, unquoted).
pub fn launch_agent_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>dev.lightspeed.gui</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20   <string>{}</string>\n\
         \x20   <string>{AUTOSTART_ARG}</string>\n\
         \x20 </array>\n\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>ProcessType</key>\n\
         \x20 <string>Interactive</string>\n\
         </dict>\n\
         </plist>\n",
        xml_escape(&exe.display().to_string())
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The `reg.exe` arguments that create the Windows Run value. Pure strings; the
/// Settings toggle's `apply_current` executes them through `silent_command`.
pub fn registry_add_args(command: &str) -> Vec<String> {
    vec![
        "add".to_string(),
        REGISTRY_RUN_KEY.to_string(),
        "/v".to_string(),
        APP_ID.to_string(),
        "/t".to_string(),
        "REG_SZ".to_string(),
        "/d".to_string(),
        command.to_string(),
        "/f".to_string(),
    ]
}

/// The `reg.exe` arguments that remove the Windows Run value (idempotent).
pub fn registry_remove_args() -> Vec<String> {
    vec![
        "delete".to_string(),
        REGISTRY_RUN_KEY.to_string(),
        "/v".to_string(),
        APP_ID.to_string(),
        "/f".to_string(),
    ]
}

/// Write a file plan under `root` (the temp dir in tests, the real config/home
/// dir in production), creating the directory chain as needed.
///
/// A registry plan is not a file: this returns an error rather than pretending
/// it wrote one. The Windows entry is the `reg.exe` value built by
/// [`registry_add_args`] and applied by [`apply_current`].
pub fn write_file(root: &Path, plan: &AutostartPlan) -> std::io::Result<PathBuf> {
    match plan {
        AutostartPlan::File {
            file_name,
            contents,
            ..
        } => {
            std::fs::create_dir_all(root)?;
            let path = root.join(file_name);
            std::fs::write(&path, contents)?;
            Ok(path)
        }
        AutostartPlan::Registry { .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the Windows autostart entry is a registry value, not a file",
        )),
    }
}

/// Remove the file entry for `os` under `root`. Returns whether a file was
/// removed; a missing file is `Ok(false)`, so disable is idempotent.
pub fn remove_file(root: &Path, os: Os) -> std::io::Result<bool> {
    for file_name in exit_file_names(os) {
        let path = root.join(file_name);
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// Whether the file entry for `os` exists under `root`.
pub fn is_installed(root: &Path, os: Os) -> bool {
    exit_file_names(os)
        .iter()
        .any(|name| root.join(name).is_file())
}

fn exit_file_names(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &[],
        Os::Linux => &["lightspeed-gui.desktop"],
        Os::Macos => &["dev.lightspeed.gui.plist"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lightspeed-autostart-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\LightSpeed\lightspeed-gui.exe")
    }

    #[test]
    fn each_os_gets_its_own_mechanism() {
        let config = PathBuf::from("/home/player/.config");
        let home = PathBuf::from("/home/player");
        assert!(matches!(
            plan(Os::Windows, &exe(), &config, &home),
            AutostartPlan::Registry { .. }
        ));
        let linux = plan(Os::Linux, &exe(), &config, &home);
        assert_eq!(
            linux.path().unwrap(),
            PathBuf::from("/home/player/.config/autostart/lightspeed-gui.desktop")
        );
        let macos = plan(Os::Macos, &exe(), &config, &home);
        assert_eq!(
            macos.path().unwrap(),
            PathBuf::from("/home/player/Library/LaunchAgents/dev.lightspeed.gui.plist")
        );
        assert_eq!(linux.kind(), "file");
        assert_eq!(macos.kind(), "file");
    }

    #[test]
    fn the_windows_command_is_a_quoted_exe_plus_the_autostart_flag() {
        let plan = plan(Os::Windows, &exe(), Path::new("."), Path::new("."));
        let AutostartPlan::Registry { command, .. } = plan else {
            panic!("windows plan is a registry value");
        };
        assert_eq!(
            command,
            "\"C:\\Program Files\\LightSpeed\\lightspeed-gui.exe\" --autostart"
        );
        let args = registry_add_args(&command);
        assert_eq!(args[0], "add");
        assert!(args.contains(&"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into()));
        assert!(args.contains(&"/v".to_string()));
        assert!(args.contains(&APP_ID.to_string()));
        assert!(args.contains(&"/f".to_string()));
        let remove = registry_remove_args();
        assert_eq!(remove[0], "delete");
        assert!(remove.contains(&APP_ID.to_string()));
    }

    #[test]
    fn the_linux_desktop_entry_quotes_the_exec_and_is_enabled() {
        let entry = desktop_entry(&exe());
        assert!(entry.starts_with("[Desktop Entry]"));
        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(!entry.contains("Hidden=true"));
        // A Windows-style backslash path is a legal (if unusual) Exec value and
        // the escaping rule is uniform: backslashes double inside the quotes.
        assert!(
            entry.contains(
                "Exec=\"C:\\\\Program Files\\\\LightSpeed\\\\lightspeed-gui.exe\" --autostart"
            ),
            "{entry}"
        );
        // A path with a quote is escaped rather than breaking the file.
        let quoted = desktop_entry(Path::new("/opt/Light \"Speed\"/gui"));
        assert!(quoted.contains("Exec=\"/opt/Light \\\"Speed\\\"/gui\" --autostart"));
    }

    #[test]
    fn the_macos_plist_is_a_launch_agent_with_escaped_arguments() {
        let plist = launch_agent_plist(Path::new("/Applications/Light & Speed/gui"));
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("<string>dev.lightspeed.gui</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<string>/Applications/Light &amp; Speed/gui</string>"));
        assert!(plist.contains(&format!("<string>{AUTOSTART_ARG}</string>")));
        // Both array elements: the exe then the flag, never one quoted string.
        assert!(!plist.contains("&quot; --autostart"));
    }

    #[test]
    fn the_toggle_path_enables_and_disables_the_entry() {
        // The Settings toggle's apply path, on a file plan under a temp root.
        // The Windows registry branch is exercised through its argument
        // builders only: a test must never write the real HKCU.
        let root = temp_root("toggle");
        let plan = AutostartPlan::File {
            dir: root.clone(),
            file_name: "lightspeed-gui.desktop",
            contents: desktop_entry(&exe()),
        };

        assert!(!is_installed(&root, Os::Linux), "off by default");
        apply_entry(&plan, Os::Linux, true).expect("enable writes the entry");
        assert!(is_installed(&root, Os::Linux), "enable installs the entry");
        assert_eq!(
            std::fs::read_to_string(plan.path().expect("file plan has a path"))
                .expect("entry text"),
            desktop_entry(&exe())
        );

        apply_entry(&plan, Os::Linux, false).expect("disable removes the entry");
        assert!(!is_installed(&root, Os::Linux), "disable removes the entry");
        // Disable is idempotent: a missing entry is not an error, so the toggle
        // can be clicked twice without a spurious failure line.
        apply_entry(&plan, Os::Linux, false).expect("disable is idempotent");
        assert!(!is_installed(&root, Os::Linux));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_registry_branch_dispatches_the_argument_builders_without_running_them() {
        // A test must never write the real HKCU login entry: the registry
        // branch is dispatched through the injectable runner and its arguments
        // are asserted here. The real `reg.exe` call is shipped in
        // `apply_current` and is only reachable from the Settings toggle.
        let root = temp_root("registry-dispatch");
        let plan = plan(Os::Windows, &exe(), &root, &root);
        let mut dispatched: Vec<Vec<String>> = Vec::new();
        {
            let mut recorder = |args: &[String]| {
                dispatched.push(args.to_vec());
                Ok(())
            };
            apply_entry_with(&plan, Os::Windows, true, &mut recorder).expect("enable dispatches");
            apply_entry_with(&plan, Os::Windows, false, &mut recorder).expect("disable dispatches");
        }
        assert_eq!(dispatched.len(), 2, "both directions dispatch one command");
        assert_eq!(dispatched[0][0], "add", "enable runs `reg add`");
        assert_eq!(dispatched[1][0], "delete", "disable runs `reg delete`");
        assert!(dispatched[0].contains(&APP_ID.to_string()));
        assert!(
            dispatched[0].contains(&launch_command(&exe())),
            "the add command carries the quoted exe + --autostart"
        );
        assert!(!root.exists(), "a registry plan never creates a file");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_file_writer_creates_and_removes_the_entry() {
        let root = temp_root("write");
        let plan = plan(
            Os::Linux,
            &exe(),
            Path::new("/tmp/ignored"),
            Path::new("/tmp/ignored"),
        );
        let path = write_file(&root, &plan).expect("write succeeds in a temp dir");
        assert!(path.is_file(), "{path:?}");
        let text = std::fs::read_to_string(&path).expect("entry text");
        assert!(text.contains("--autostart"));
        assert!(is_installed(&root, Os::Linux));
        assert!(remove_file(&root, Os::Linux).expect("remove succeeds"));
        assert!(!is_installed(&root, Os::Linux));
        // Disable is idempotent: removing again is `false`, not an error.
        assert!(!remove_file(&root, Os::Linux).expect("second remove is a no-op"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_writer_surfaces_failures_instead_of_swallowing_them() {
        // A parent path that is a *file* cannot become a directory: the writer
        // must report it, not silently claim success.
        let root = temp_root("failure");
        std::fs::create_dir_all(&root).expect("create temp root");
        let blocker = root.join("blocked");
        std::fs::write(&blocker, "not a directory").expect("write blocker file");
        let plan = plan(Os::Macos, &exe(), &root, &root);
        let result = write_file(&blocker.join("LaunchAgents"), &plan);
        assert!(result.is_err(), "a file in the way must be an error");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_registry_plan_is_never_written_as_a_file() {
        let root = temp_root("registry");
        let plan = plan(Os::Windows, &exe(), &root, &root);
        let result = write_file(&root, &plan);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Unsupported);
        // Nothing was created as a side effect.
        assert!(!root.exists());
    }

    #[test]
    fn the_startup_plan_resolves_on_this_host() {
        let plan = plan_for_current();
        // On a host where the exe path is known this must be the platform's
        // mechanism; a failure to resolve is reported (not retried).
        if let Some(plan) = plan {
            match current_os() {
                Os::Windows => assert_eq!(plan.kind(), "registry"),
                _ => assert_eq!(plan.kind(), "file"),
            }
            assert!(!plan.describe().is_empty());
        }
    }
}
