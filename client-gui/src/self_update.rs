//! In-place installer for receipt-less installs.
//!
//! The availability check ([`crate::update`]) works everywhere, but replacing
//! the running binary in place is Windows-only here: the Program Files copy is
//! how this GUI ships on Windows, and self-replacement needs elevation, which
//! PowerShell's RunAs verb provides. On other platforms the user updates
//! through their package manager, and the install path says so.
//!
//! The flow, end to end: download the zip and its `.sha256` sibling, verify
//! the checksum BEFORE anything runs elevated, then hand a pre-written
//! PowerShell script to an elevated shell that stops the app, extracts the
//! archive with the built-in `tar` (Windows ships bsdtar, which reads zips),
//! copies the new exe over the current one, verifies the copy by size, and
//! relaunches. The calling process exits after the hand-off.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::update::ReleaseAsset;

/// Progress pushed to the UI: a stage label, or a failure.
pub type InstallOutcome = Result<String, String>;

/// Run the whole install on the current thread. Blocking; the GUI calls this
/// from a background thread.
pub fn install_blocking(asset: ReleaseAsset) -> InstallOutcome {
    #[cfg(not(windows))]
    {
        let _ = asset;
        return Err(
            "in-place updates run on Windows; update through your package manager".to_string(),
        );
    }

    #[cfg(windows)]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("failed to create tokio runtime: {e}"))?;
        runtime.block_on(install(asset))
    }
}

#[cfg(windows)]
async fn install(asset: ReleaseAsset) -> InstallOutcome {
    let client = crate::update::http_client()?;

    let exe = std::env::current_exe().map_err(|e| format!("cannot locate this binary: {e}"))?;
    let exe_name = exe
        .file_name()
        .ok_or_else(|| "current binary has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let dest_dir = exe
        .parent()
        .ok_or_else(|| "current binary has no parent directory".to_string())?
        .to_path_buf();

    let tmp = std::env::temp_dir().join(format!("lightspeed-update-{}", asset.tag));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {tmp:?}: {e}"))?;
    let zip_path = tmp.join("lightspeed-gui.zip");

    // Download and verify BEFORE anything runs elevated.
    let zip_bytes = fetch(&client, &asset.zip_url).await?;
    let sha_text = fetch(&client, &asset.sha256_url).await?;
    let expected = parse_sha256_file(&String::from_utf8_lossy(&sha_text))?;

    let actual = {
        let mut hasher = Sha256::new();
        hasher.update(&zip_bytes);
        format!("{:x}", hasher.finalize())
    };
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "checksum mismatch: release says {expected}, download is {actual} - not installing"
        ));
    }
    std::fs::write(&zip_path, &zip_bytes).map_err(|e| format!("cannot write {zip_path:?}: {e}"))?;

    let script = swap_script(&zip_path, &dest_dir, &exe_name);
    let script_path = tmp.join("apply-update.ps1");
    std::fs::write(&script_path, script)
        .map_err(|e| format!("cannot write {script_path:?}: {e}"))?;

    spawn_elevated(&script_path)?;
    Ok("handed off to the elevated installer; this process will exit".to_string())
}

#[cfg(windows)]
async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed for {url}: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download failed for {url}: HTTP {status}"));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("download failed for {url}: {e}"))
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

/// The elevated PowerShell script that performs the swap. Pure so a test can
/// assert its contract: stop any running instance, extract the verified zip
/// with the built-in `tar` (bsdtar reads zips), copy the new exe over the
/// current one, verify the copy landed by size, relaunch. Also refreshes
/// WinDivert files when the archive carries them (the manual install layout
/// keeps them beside the exe).
fn swap_script(zip: &Path, dest_dir: &Path, exe_name: &str) -> String {
    let zip = zip.display();
    let dest = dest_dir.display();
    format!(
        r#"$ErrorActionPreference = 'Stop'
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | ForEach-Object {{ $_.Kill(); $_.WaitForExit(8000) | Out-Null }}
Start-Sleep -Seconds 2
$extract = Join-Path $env:TEMP 'lightspeed-update-extract'
if (Test-Path $extract) {{ Remove-Item $extract -Recurse -Force }}
New-Item -ItemType Directory -Path $extract | Out-Null
tar -xf '{zip}' -C $extract
$new = Get-ChildItem -Path $extract -Recurse -Filter '*.exe' | Where-Object {{ $_.Name -eq '{exe_name}' }} | Select-Object -First 1
if (-not $new) {{ throw 'the update archive contains no {exe_name}' }}
$dst = Join-Path '{dest}' '{exe_name}'
Copy-Item $new.FullName $dst -Force
if ((Get-Item $dst).Length -ne $new.Length) {{ throw 'copy did not land (size mismatch)' }}
Get-ChildItem -Path $extract -Recurse -Include 'WinDivert*.dll','WinDivert*.sys' -ErrorAction SilentlyContinue | ForEach-Object {{ Copy-Item $_.FullName '{dest}' -Force }}
Start-Process $dst
"#
    )
}

/// Spawn the swap script elevated. The RunAs verb is the only way to raise a
/// child's integrity from a medium process; from an already-elevated process
/// it spawns without a UAC prompt, otherwise the user approves once.
#[cfg(windows)]
fn spawn_elevated(script: &Path) -> Result<(), String> {
    let quoted = script.display().to_string().replace('\'', "''");
    let inner = format!(
        "Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{quoted}'"
    );
    crate::platform::silent_command("powershell")
        .args(["-WindowStyle", "Hidden", "-Command", &inner])
        .spawn()
        .map_err(|e| format!("failed to launch the elevated installer: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_sha256_file, swap_script};
    use std::path::Path;

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

    #[test]
    fn swap_script_carries_the_contract() {
        let script = swap_script(
            Path::new("C:/tmp/lightspeed-gui.zip"),
            Path::new("C:/Program Files/lightspeed-gui/bin"),
            "lightspeed-gui.exe",
        );
        for needle in [
            "tar -xf 'C:/tmp/lightspeed-gui.zip'",
            "C:/Program Files/lightspeed-gui/bin",
            "lightspeed-gui.exe",
            "Get-Process lightspeed-gui",
            "size mismatch",
            "Start-Process $dst",
        ] {
            assert!(
                script.contains(needle),
                "script missing {needle:?}\n{script}"
            );
        }
    }
}
