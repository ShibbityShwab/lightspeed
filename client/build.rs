//! Copies the bundled WinDivert runtime files next to the built binary.
//!
//! `client/src/interceptor/windows.rs` requires `WinDivert.dll` and
//! `WinDivert64.sys` to sit beside the executable, because WinDivert resolves
//! both from the process's own directory. Release packaging already bundles
//! them (`[package.metadata.dist] include` in `Cargo.toml`), but a local
//! `cargo build` did not, so an interception attempt under `cargo run` or a
//! debug build failed at startup with "WinDivert64.sys not found" even though
//! the file is vendored in the repository.
//!
//! This runs on every build, is a no-op off Windows, and never fails the build:
//! a missing or unreadable source file is reported as a warning so the failure
//! surfaces where it is actionable (the interceptor's own availability check),
//! not as a compile error in an unrelated crate.

use std::path::Path;

/// Files the interceptor opens next to the executable, in copy order.
const RUNTIME_FILES: [&str; 2] = ["WinDivert.dll", "WinDivert64.sys"];

fn main() {
    println!("cargo:rerun-if-changed=windivert/WinDivert.dll");
    println!("cargo:rerun-if-changed=windivert/WinDivert64.sys");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // `OUT_DIR` is `<target-dir>/<profile>/build/<pkg>-<hash>/out`, so the
    // directory holding the executable is three levels up.
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let Some(exe_dir) = Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };

    for name in RUNTIME_FILES {
        let src = Path::new("windivert").join(name);
        if !src.exists() {
            println!(
                "cargo:warning=bundled {name} is missing from client/windivert/, so \
                 Windows interception will refuse to start from this build"
            );
            continue;
        }
        let dst = exe_dir.join(name);
        // Skip the copy when the destination is already current, so a rebuild
        // does not fight a driver that is loaded in the kernel.
        let up_to_date = match (src.metadata(), dst.metadata()) {
            (Ok(s), Ok(d)) => match (s.modified(), d.modified()) {
                (Ok(st), Ok(dt)) => dt >= st,
                _ => false,
            },
            _ => false,
        };
        if up_to_date {
            continue;
        }
        if let Err(e) = std::fs::copy(&src, &dst) {
            println!("cargo:warning=could not place {name} next to the binary: {e}");
        }
    }
}
