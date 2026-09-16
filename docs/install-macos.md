# Install LightSpeed on macOS

macOS is a CLI-first platform. The GUI compiles for macOS but is untested on real hardware, so the command-line client (`lightspeed-client`) is the supported path.

---

## What to download

Grab the latest release from the [Releases page](https://github.com/ShibbityShwab/lightspeed/releases/latest). Pick the archive that matches your Mac's CPU:

| Your Mac | Target | File |
|----------|--------|------|
| Apple Silicon (M1 and later) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

Not sure which you have? Run:

```bash
uname -m
```

`arm64` means Apple Silicon; `x86_64` means Intel.

---

## Install

The shell installer is the easiest path. It detects your architecture and installs the client:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Or install manually from the archive:

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Verify the binary runs:

```bash
lightspeed-client --version
```

---

## Root privileges

The macOS interceptor uses `pfctl` (the built-in packet filter) to redirect game UDP traffic. `pfctl` requires root, so run the client with `sudo` when you start the interceptor:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Discovery, probing, and diagnostics (`--probe-proxies`, `--test-control`, `--check`) do not need root.

---

## First run

The client discovers the community relays automatically through the signed registry. No proxy address is needed.

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## Verify it works

**Check relay discovery.** Run:

```bash
lightspeed-client --probe-proxies
```

This performs one discovery/probe pass and prints a visible report listing each discovered relay and its latency. You should see all five community relays (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo).

**Check control-plane registration.** Run:

```bash
lightspeed-client --test-control
```

This connects to the QUIC control plane, registers a session, pings, and disconnects, printing the result of each step. A successful registration proves the control plane is reachable and auth is working.

**Check the environment.** Run:

```bash
lightspeed-client --check
```

This reports interceptor availability, root status, game detection, and proxy reachability.

**Check packet flow.** Once the interceptor is running and your game is connected to a server, the client's packet counters should climb. If "Packets Sent" stays at 0, the interceptor has not seen game traffic yet; see [Troubleshooting](troubleshooting.md).

---

## Gatekeeper note

If macOS blocks the binary with "cannot be opened because the developer cannot be verified", clear the quarantine attribute:

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Only do this if you downloaded the binary from the official Releases page.

---

## Next steps

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
