# LightSpeed unter macOS installieren

> [!WARNING]
> Maschinell unterstützte Übersetzung, nicht von Muttersprachlern geprüft. Maßgeblich ist die [deutsche Fassung](install-macos.md).

macOS ist eine CLI-first-Plattform. Die GUI lässt sich für macOS kompilieren, ist aber auf echter Hardware ungetestet, der Kommandozeilen-Client (`lightspeed-client`) ist also der unterstützte Weg.

---

## Was du herunterladen musst

Hol dir das neueste Release von der [Releases-Seite](https://github.com/ShibbityShwab/lightspeed/releases/latest). Nimm das Archiv, das zur CPU deines Macs passt:

| Dein Mac | Ziel | Datei |
|----------|--------|------|
| Apple Silicon (M1 und neuer) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

Nicht sicher, welchen du hast? Führe aus:

```bash
uname -m
```

`arm64` bedeutet Apple Silicon; `x86_64` bedeutet Intel.

---

## Installation

Der Shell-Installer ist der einfachste Weg. Er erkennt deine Architektur und installiert den Client:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Oder installiere manuell aus dem Archiv:

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Prüfe, ob die Binary läuft:

```bash
lightspeed-client --version
```

---

## Root-Rechte

Der macOS-Interceptor nutzt `pfctl` (den eingebauten Paketfilter), um den UDP-Spielverkehr umzuleiten. `pfctl` braucht root, führe den Client also mit `sudo` aus, wenn du den Interceptor startest:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Erkennung, Sondierung und Diagnose (`--probe-proxies`, `--test-control`, `--check`) brauchen kein root.

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

## Hinweis zum Gatekeeper

Blockiert macOS die Binary mit "cannot be opened because the developer cannot be verified", entferne das Quarantäne-Attribut:

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Mach das nur, wenn du die Binary von der offiziellen Releases-Seite heruntergeladen hast.

---

## Nächste Schritte

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
