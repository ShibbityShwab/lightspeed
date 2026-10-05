# LightSpeed unter Linux installieren

> [!WARNING]
> Maschinell unterstützte Übersetzung, nicht von Muttersprachlern geprüft. Maßgeblich ist die [deutsche Fassung](install-linux.md).

Linux ist eine CLI-first-Plattform. Die GUI lässt sich für Linux bauen, aber der Kommandozeilen-Client (`lightspeed-client`) ist der unterstützte Weg für Headless- und Power-User-Setups.

---

## Was du herunterladen musst

Hol dir das neueste Release von der [Releases-Seite](https://github.com/ShibbityShwab/lightspeed/releases/latest). Nimm das Archiv, das zu deiner CPU passt:

| Deine Maschine | Ziel | Datei |
|--------------|--------|------|
| x86_64 (Intel/AMD) | `x86_64-unknown-linux-gnu` | `lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz` |
| ARM64 (Ampere, Graviton, Raspberry Pi 4/5) | `aarch64-unknown-linux-gnu` | `lightspeed-client-...-aarch64-unknown-linux-gnu.tar.xz` |

Nicht sicher, welchen du hast? Führe aus:

```bash
uname -m
```

`x86_64` bedeutet 64-Bit Intel/AMD; `aarch64` oder `arm64` bedeutet ARM64.

---

## Installation

Der Shell-Installer ist der einfachste Weg. Er erkennt deine Architektur und installiert den Client:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Oder installiere manuell aus dem Archiv:

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Prüfe, ob die Binary läuft:

```bash
lightspeed-client --version
```

---

## Root-Rechte

Der Linux-Interceptor nutzt `nftables` (oder `iptables`), um den UDP-Spielverkehr umzuleiten, was root erfordert. Führe den Client mit `sudo` aus, wenn du den Interceptor startest:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Erkennung, Sondierung und Diagnose (`--probe-proxies`, `--test-control`, `--check`) brauchen kein root.

Stelle sicher, dass `nftables` installiert ist, falls deine Distribution es nicht standardmäßig mitbringt:

```bash
# Debian/Ubuntu
sudo apt install nftables

# Fedora/RHEL
sudo dnf install nftables

# Arch
sudo pacman -S nftables
```

---

## Erster Start

Der Client findet die Community-Relays automatisch über das signierte Register. Keine Proxy-Adresse nötig.

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## Prüfen, ob es funktioniert

**Prüfe die Relay-Erkennung.** Führe aus:

```bash
lightspeed-client --probe-proxies
```

Das führt einen Durchlauf aus Erkennung und Sondierung aus und gibt einen sichtbaren Bericht aus, der jedes gefundene Relay und seine Latenz auflistet. Du solltest alle acht Community-Relays sehen (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney).

**Prüfe die Control-Plane-Registrierung.** Führe aus:

```bash
lightspeed-client --test-control
```

Das verbindet sich mit der QUIC-Control-Plane, registriert eine Sitzung, pingt und trennt wieder, und gibt das Ergebnis jedes Schritts aus. Eine erfolgreiche Registrierung beweist, dass die Control-Plane erreichbar ist und die Authentifizierung funktioniert.

**Prüfe die Umgebung.** Führe aus:

```bash
lightspeed-client --check
```

Das meldet Interceptor-Verfügbarkeit, Root-Status, Spielerkennung und Proxy-Erreichbarkeit.

**Prüfe den Paketfluss.** Sobald der Interceptor läuft und dein Spiel mit einem Server verbunden ist, sollten die Paketzähler des Clients steigen. Bleibt "Packets Sent" bei 0, hat der Interceptor noch keinen Spielverkehr gesehen; siehe [Troubleshooting](troubleshooting.md).

---

## Nächste Schritte

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
