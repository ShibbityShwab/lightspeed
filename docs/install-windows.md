# Install LightSpeed on Windows

Windows is the GUI-first platform. The `lightspeed-gui` package is a standalone app that already contains the client engine and the WinDivert driver, so you never need a separate client download.

---

## What to download

Grab the latest release from the [Releases page](https://github.com/ShibbityShwab/lightspeed/releases/latest). For Windows x86_64 (all current Intel and AMD PCs), pick one of:

| File | Use it when |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | Recommended. Installs to Program Files, adds a Start Menu shortcut, and registers an uninstaller. |
| `lightspeed-gui-...-windows-msvc.zip` | Portable. Unzip anywhere and run `lightspeed-gui.exe`. |

Both contain the same GUI plus `WinDivert.dll` and `WinDivert64.sys`.

> **Windows on ARM64 is not a published target yet.** The release builds target `x86_64-pc-windows-msvc`. On an ARM64 device, the x86_64 build runs under Windows' emulation layer, but it is untested on real ARM64 hardware.

### Windows CLI (unsupported)

A Windows command-line build is also published as a zip (`lightspeed-client-...-windows-msvc.zip`). It is provided for scripting and headless use, but it is **unsupported**: the GUI is the recommended Windows path. If you use it, run it from an elevated terminal (see below).

---

## Install (MSI)

1. Download the `.msi`.
2. Double-click it and follow the wizard. Windows SmartScreen may warn about an unknown publisher; the release is Sigstore-attested, so you can verify the attestation if you want to.
3. Launch **LightSpeed** from the Start Menu.

## Install (portable zip)

1. Download the `.zip`.
2. Right-click it, choose **Extract All**, and extract to a folder you can write to (for example `C:\LightSpeed`). Do not run it from inside the zip.
3. Run `lightspeed-gui.exe`.

---

## Administrator privileges

LightSpeed uses the WinDivert driver to intercept game UDP traffic. WinDivert requires **Administrator** rights.

- The GUI requests elevation when it needs to start the interceptor. Accept the UAC prompt.
- If you run the CLI build, launch it from an **Administrator** terminal (right-click Windows Terminal or PowerShell, then **Run as administrator**).

Without elevation, the interceptor cannot attach and you will see an interceptor error in the status view.

---

## First run

1. Launch the GUI and accept the UAC prompt.
2. The GUI discovers the community relays automatically through the signed registry. No proxy address is needed.
3. Pick your game from the game list.
4. Start the interceptor, then launch your game and connect to a server.

The GUI shows relay status, registration state, and packet counters so you can see traffic flowing.

---

## Verify it works

**Check relay discovery and registration.** Open the GUI status view. You should see the community relays listed (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo) with a healthy status, and a registration line showing the QUIC/auth handshake succeeded. If you are using the CLI build, run:

```powershell
lightspeed-client.exe --probe-proxies
```

This performs one discovery/probe pass and prints a visible report listing each discovered relay and its latency. You should see all seven community relays.

**Check control-plane registration.** With the CLI build:

```powershell
lightspeed-client.exe --test-control
```

This connects to the QUIC control plane, registers a session, pings, and disconnects, printing the result of each step. A successful registration proves the control plane is reachable and auth is working.

**Check packet flow.** In the GUI, the packet counters should climb once your game is connected to a server. If "Packets Sent" stays at 0, the interceptor has not seen game traffic yet; see [Troubleshooting](troubleshooting.md).

---

## Logs

The GUI writes a trace log to:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Paste this path into the File Explorer address bar to open it. Attach it to a bug report if something goes wrong. See [Troubleshooting](troubleshooting.md) for what the log lines mean.

---

## Uninstall

- **MSI:** Settings → Apps → Installed apps → LightSpeed → Uninstall.
- **Zip:** delete the extracted folder. The log file under `%LOCALAPPDATA%\Lightspeed\` is left behind; delete it manually if you want a clean removal.

---

## Next steps

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
