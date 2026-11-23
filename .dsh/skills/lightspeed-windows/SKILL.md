---
name: lightspeed-windows
description: Use when running commands, builds, tests, scripts or GUI checks for LightSpeed on this Windows development host, or when a command fails for a platform reason. Covers PowerShell 5.1 limits, the real toolchain, elevation and single-instance quirks, missing Python/bash, and which work must run remotely.
---

# LightSpeed on Windows

> Paths in this document are relative to the repository root, not to this skill directory.

This host is the primary development machine. Everything below was observed on
it; do not substitute a POSIX habit.

## Shell

- The only shell is **Windows PowerShell 5.1**. `pwsh` is not installed.
  - No `??`, no ternary, no `ProcessStartInfo.ArgumentList`, no `$PSVersionTable`
    workarounds that assume 7.x.
  - Check exit codes with `$LASTEXITCODE` after native commands; use
    `-ErrorAction` for cmdlets.
- **No Git Bash, no WSL, no `bash script.sh`.** `git.exe` is a Windows build and
  works directly.
- Run commands with the `pwsh` tool. Do not `cd` in one call and expect the next
  call to remember it; pass `workdir` instead.
- `cmd`/`.bat` files stay CRLF (`.gitattributes`); all other text is LF, so a
  fresh `git checkout` and the `edit` tool agree.

## Toolchain reality

| Tool | State |
|---|---|
| `cargo` / `rustc` | **Installed**, 1.100.0 via `rust-toolchain.toml`; host `x86_64-pc-windows-msvc` |
| `cargo fmt` / `cargo clippy` | Installed (rustup components) |
| `cargo audit` / `cargo deny` | **Missing** — `cargo install cargo-audit cargo-deny`, or run `Check-LightSpeed.ps1 -SkipAudit`; CI's security workflow owns both gates |
| `git` | Installed (`C:\Program Files\Git\cmd\git.exe`) |
| `node` | Installed |
| `gh`, `winget` | Installed |
| `python` / `python3` | **Microsoft Store stubs** — `python -c ...` exits 9009 |
| `py` / `python` (real) | **CPython 3.12.10 installed** at `%LOCALAPPDATA%\Programs\Python\Python312`; `py` is on the user PATH, so a shell started before 2026-11-16 needs a restart to see it |
| `pnpm`, `docker`, `choco`, `scoop` | Missing |

Consequences:

- Localization and `tools/*.py` run with `py -3 ...` or
  `scripts/Localization.ps1` (which resolves the interpreter). If `py` is not
  found, pass `-Python` the full path; the Store aliases are still not real
  interpreters.
- Relay operations (`infra/scripts/*.sh`) are Linux artifacts. They run on the
  relay hosts and in CI, never on this desktop.

## Build and test gates

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets      # GUI is excluded above
cargo test -p lightspeed-gui
cargo test -p lightspeed-client --features windivert-redirect
```

`tools/Check-LightSpeed.ps1` runs the pre-push set; `tools/check.cmd` is the same
gate for a plain `cmd` prompt. The GUI crate is excluded from workspace clippy on
every platform — lint it explicitly on Windows, which is the only platform where
it builds.

## GUI quirks

- A single-instance mutex is shared across **all** builds: a stray
  `target/debug` process silently wins over an installed launch. Check
  `Get-Process lightspeed-gui` before launching; `scripts/dev-gui.ps1` guards it.
- Elevation does not propagate into the agent shell. A medium-integrity shell
  cannot stop an elevated GUI (`Stop-Process -Force` fails "Could not stop
  process"), and a running GUI locks its own exe (`Access is denied (os error 5)`
  on rebuild). Use `scripts/dev-gui.ps1 -Elevate`, stop from the tray, and verify
  installs by file size, not exit code (`scripts/install-gui.ps1` already does).
- egui in-window Modal sheets ARE exposed to UI Automation (verified 2026-11-23
  with `scripts/uia-tree.ps1` + `Invoke`); inline combo popups are exposed (live-verified 2026-11-23; option buttons
  without stable ids), context menus are untested. Verify a collapsed control's
  layout by its selected text and a screenshot.
- Launch a GUI build with `Start-Process` and keep the handle; do not leave
  console windows behind.

## Sandbox quirk: workspace binaries run at Low integrity (diagnosed 2026-11-17)

A test binary that lives under the workspace (`target\...`) can be denied when it
creates files outside it: `cargo test` panics with `Access is denied` on
`%TEMP%\...` while the **same binary copied to `%TEMP%` succeeds**.

Diagnosed cause: the workspace tree is **Low-integrity labelled**, so an exe
built into `target` starts at Low Integrity (Mandatory Integrity Control) and
cannot write to a Medium-integrity location such as `%TEMP%`, `%APPDATA%` or
`Documents`. The same exe copied to `%TEMP%` starts at Medium and can. Proof:
`whoami /groups` printed by the binary shows `Mandatory Label\Low Mandatory
Level` from `target\...` and `Medium Mandatory Level` from the `%TEMP%` copy;
.exe files in `C:\Windows\System32` (`cmd.exe`, `powershell.exe`) run Medium, so
their writes succeed - that asymmetry is the tell.

**Verified workaround (keeps tests in the workspace):** point the child temp dir
inside the workspace before the test run, so the Low process writes to Low:

```powershell
New-Item -ItemType Directory -Force target\test-temp | Out-Null
$env:TEMP = (Join-Path (Get-Location) 'target\test-temp')
$env:TMP  = $env:TEMP
cargo test --workspace --all --exclude lightspeed-gui
```

With that, the 2026-11-17 gate was **1,738 workspace tests / 0 failed / 7 ignored
and 181 GUI / 0 / 1** on this host. Without it, every test that calls
`std::env::temp_dir()` fails with os error 5 - an environment failure, not a
code failure. Building with `CARGO_TARGET_DIR` outside the workspace is the
other workaround. Do not "fix" the tests for this.

## Evidence

Screenshots, command transcripts and benchmark tables go to
`wat/evidence/<YYYY-MM-DD>/` (gitignored) and are named in the task report and
the state entry. "It worked" without a path is unverified.

## When the host cannot do it

Say so explicitly and route it:

- Linux relay work → the relay host over SSH, or `infra/` CI.
- macOS packaging → a macOS runner or contributor.
- Python-dependent scripts → install CPython (owner decision) or run in CI.
- Sandbox denials → they are DSH policy; escalate the exact operation once with
  `sandbox_permissions` rather than finding another route.
