# LightSpeed Benutzerhandbuch

> [!WARNING]
> Maschinell unterstützte Übersetzung, nicht von Muttersprachlern geprüft. Maßgeblich ist die [deutsche Fassung](user-guide.md).

> Schritt-für-Schritt-Anleitung, um deinen Ping mit LightSpeed zu senken.

---

## Wie LightSpeed funktioniert

Dein ISP leitet den Spielverkehr über Pfade, die auf Kosten statt auf Geschwindigkeit optimiert sind. LightSpeed fängt die UDP-Pakete deines Spiels ab und tunnelt sie durch einen **Relay**, einen schlanken Server in einem Rechenzentrum mit schnellen Backbone-Verbindungen zu den Regionen der Spielserver. Standardmäßig nutzt du das **Community-Relay-Netzwerk** (acht von Sponsoren finanzierte Relays, automatisch über ein signiertes Register gefunden, keine Einrichtung nötig). Du kannst auch deinen eigenen Proxy selbst hosten. Ist dieser Pfad schneller als die Standardroute deines ISP, sinkt dein Ping.

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## Voraussetzungen

- Das `lightspeed` CLI-Tool oder `lightspeed-gui` (Windows). Es ist keine Proxy-Einrichtung nötig: der Client findet die Community-Relays automatisch.
- Für den Interceptor-Modus: root-/Administrator-Rechte
- Optional: dein eigener Proxy-Node, falls du selbst hosten willst (siehe [Deploy Proxy](deploy-proxy.md))

---

## Welche App brauche ich?

| Du bist auf | Download | Warum |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui` (MSI oder ZIP) | Die GUI ist eine eigenständige App, sie enthält bereits die Client-Engine + den WinDivert-Treiber. Du brauchst das CLI **nicht**. |
| **Linux** | `lightspeed-gui` (oder `lightspeed-client`) | Die GUI läuft auf Linux (das System-Tray ist ein Platzhalter); das CLI ist für Power-User. |
| **macOS** | `lightspeed-client` | Noch keine getestete GUI. Die GUI lässt sich für macOS kompilieren, ist aber auf echter Hardware **ungetestet**. |
| **Proxy hosten** | `lightspeed-proxy` | Nur wenn du einen Relay-Node auf einem VPS betreibst. |

> **Du brauchst immer nur ein Paket.** Bist du Windows-Spieler, nimm `lightspeed-gui` und ignoriere den Rest. Das `lightspeed-client` ist für Linux-Power-User und macOS-Spieler; `lightspeed-proxy` ist für Selbst-Hoster.

---

## Schnellstart (CLI - alle Plattformen)

### 1. Prüfe deine Umgebung

```bash
lightspeed --check
```

Das prüft, ob dein Betriebssystem die nötigen Paketfilter-Werkzeuge hat (nftables/iptables unter Linux, pfctl unter macOS, WinDivert unter Windows).

### 2. Sondiere deine Relays

```bash
lightspeed --probe-proxies
```

Zeigt die Latenz zu jedem gefundenen Relay. Der Client wählt beim ersten Start automatisch den schnellsten aus; du kannst das übergehen, indem du den wählst, der deinem **Spielserver** am nächsten liegt, nicht deinem Standort.

### 3. Starte den Interceptor

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. Starte dein Spiel

Verbinde dich ganz normal mit einem Server. LightSpeed erkennt den Spielserver automatisch an den ausgehenden Paketen und beginnt innerhalb von Sekunden zu tunneln.

### 5. Beobachte

Das CLI zeigt Live-Statistiken:
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## Schnellstart (GUI - Windows)

### 1. Herunterladen

Hol dir das neueste Release von [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Entpacke alle Dateien, halte `WinDivert64.sys` und `WinDivert.dll` neben `lightspeed-gui.exe`.

### 2. Als Administrator ausführen

Rechtsklick auf `lightspeed-gui.exe` → **Als Administrator ausführen**. Der Interceptor braucht Zugriff auf Kernel-Ebene (genau wie VPN-Software).

### 3. Wähle ein Relay und ein Spiel

Die GUI findet die Community-Relays und wählt beim ersten Start automatisch den schnellsten aus. Du kannst das Relay im Dropdown ändern und dann dein Spiel wählen.

### 4. Klicke auf **⚡ OPTIMIZE MY ROUTE**

Der Status wechselt zu "🎯 Finding your game server…"

### 5. Starte dein Spiel

Verbinde dich mit einem beliebigen Server. LightSpeed erkennt ihn innerhalb von Sekunden.

---

### macOS GUI (ungetestet)

Die GUI lässt sich für macOS kompilieren, ist aber auf echter Hardware **ungetestet**. Das Release
liefert ein bloßes `tar.xz` aus (cargo-dist 0.32 hat keine `.app`/`.dmg`-Unterstützung), um also
ein richtiges Bundle zu erzeugen, führe auf einem Mac aus:

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

Das erzeugt `LightSpeed.app` und `LightSpeed-1.6.5.dmg`. Die App ist ad-hoc
signiert, der erste Start braucht also Rechtsklick → Öffnen (oder
`xattr -dr com.apple.quarantine LightSpeed.app`).

---

## Das richtige Relay wählen

| Du bist in | Spielserver in | Beste Relay-Region |
|-----------|---------------|-------------------|
| Australien | US West | US West (Los Angeles) |
| Europa | US East | US East (New Jersey) |
| Südostasien | Singapur | Singapur |
| Südasien | Indien | Mumbai |
| Ostasien | Japan | Tokio |
| Südamerika | US East | US East (New Jersey) |
| Überall | Gleiche Region | Am nächsten zum Spielserver |

> **Faustregel:** Wähle das Relay, das dem **Spielserver** am nächsten liegt, nicht dir. Dein Verkehr läuft PC → Relay → Spielserver, also zählt die Strecke Relay-zu-Spielserver am meisten.

---

## Forward Error Correction (FEC)

FEC fügt rund 25% Bandbreiten-Overhead hinzu, um verlorene Pakete ohne Neuübertragung wiederherzustellen.

**Aktiviere es, wenn:**
- du Paketverlust hast (Mikroruckler, Rubber-Banding)
- du per WLAN mit zeitweisen Störungen verbunden bist

**Deaktiviere es, wenn:**
- deine Verbindung ohnehin schon ausgelastet ist
- du eine Verbindung mit Datenlimit hast
- du unter 0,1% Paketverlust hast (kein Nutzen)

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## Fortgeschritten: Manueller Servermodus

Wenn die automatische Erkennung nicht funktioniert (eigene Ports, ungewöhnliches Spiel):

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

Konfiguriere dann dein Spiel so, dass es sich mit `127.0.0.1:<port>` verbindet (der lokale Port, den LightSpeed ausgibt).

---

## Server mitten in der Sitzung wechseln

LightSpeed erkennt automatisch, wenn du dich von einem Server trennst und mit einem anderen verbindest. Der Status zeigt kurz "🎯 Finding your game server…" und rastet auf das neue Ziel ein. Kein manueller Eingriff nötig.

---

## System-Tray (Windows GUI)

- Klicke auf **×**, um in den Tray zu minimieren (beendet die App nicht)
- Doppelklick auf das Blitz-Symbol stellt das Fenster wieder her
- Rechtsklick für schnelles Verbinden / Trennen / Beenden

---

## Siehe auch

- [CLI Reference](CLI-REFERENCE.md) - jeder Schalter erklärt
- [FAQ](faq.md) - häufige Fragen
- [Troubleshooting](troubleshooting.md) - Probleme beheben
- [Deploy Proxy](deploy-proxy.md) - eigenen Proxy betreiben
- [Supported Games](supported-games.md) - Spielkompatibilität
