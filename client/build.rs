/// Copies the bundled WinDivert files (WinDivert.dll + WinDivert64.sys) next to
/// the compiled binary so that `wix/main.wxs` — which sources them from
/// `$(var.CargoTargetBinDir)` — can include them in the MSI installer.
#[cfg(target_os = "windows")]
fn copy_windivert_binaries() {
    use std::env;
    use std::path::{Path, PathBuf};

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // OUT_DIR = <target>/<triple>/<profile>/build/<pkg>-<hash>/out.
    // The compiled binaries live three parents up, in <target>/<triple>/<profile>.
    let bin_dir = match out_dir.ancestors().nth(3) {
        Some(dir) => dir.to_path_buf(),
        None => return,
    };

    for name in ["WinDivert.dll", "WinDivert64.sys"] {
        let src = manifest_dir.join("windivert").join(name);
        let dst = bin_dir.join(name);
        if src.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }

    println!("cargo:rerun-if-changed=windivert/WinDivert.dll");
    println!("cargo:rerun-if-changed=windivert/WinDivert64.sys");
}

fn main() {
    #[cfg(target_os = "windows")]
    copy_windivert_binaries();
}
