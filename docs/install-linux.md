# Install LightSpeed on Linux

Linux is a CLI-first platform. The GUI builds for Linux, but the command-line client (`lightspeed-client`) is the supported path for headless and power-user setups.

---

## What to download

Grab the latest release from the [Releases page](https://github.com/ShibbityShwab/lightspeed/releases/latest). Pick the archive that matches your CPU:

| Your machine | Target | File |
|--------------|--------|------|
| x86_64 (Intel/AMD) | `x86_64-unknown-linux-gnu` | `lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz` |
| ARM64 (Ampere, Graviton, Raspberry Pi 4/5) | `aarch64-unknown-linux-gnu` | `lightspeed-client-...-aarch64-unknown-linux-gnu.tar.xz` |

Not sure which you have? Run:

```bash
uname -m
```

`x86_64` means 64-bit Intel/AMD; `aarch64` or `arm64` means ARM64.

---

## Install

The shell installer is the easiest path. It detects your architecture and installs the client:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Or install manually from the archive:

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Verify the binary runs:

```bash
lightspeed-client --version
```

---

## Root privileges

The Linux interceptor uses `nftables` (or `iptables`) to redirect game UDP traffic, which requires root. Run the client with `sudo` when you start the interceptor:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Discovery, probing, and diagnostics (`--probe-proxies`, `--test-control`, `--check`) do not need root.

Make sure `nftables` is installed if your distribution does not ship it by default:

```bash
# Debian/Ubuntu
sudo apt install nftables

# Fedora/RHEL
sudo dnf install nftables

# Arch
sudo pacman -S nftables
```

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

This performs one discovery/probe pass and prints a visible report listing each discovered relay and its latency. You should see all eight community relays (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney).

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

## Next steps

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
