//! Console-safe process spawning.
//!
//! The GUI runs with `windows_subsystem = "windows"` and owns no console.
//! When a console binary (tasklist, netstat, netsh, powershell - all used by
//! this crate on the GUI's paths) is spawned without `CREATE_NO_WINDOW`,
//! Windows allocates a NEW console window for the child, so every game scan or
//! firewall call flashed a terminal over the game. Everything in the GUI's
//! process tree spawns through [`silent_command`] instead.

/// A [`std::process::Command`] for `program` that cannot flash a console
/// window on Windows.
///
/// On non-Windows targets this is exactly `Command::new`.
pub fn silent_command(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the child runs without a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
