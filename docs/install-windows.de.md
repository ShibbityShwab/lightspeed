# LightSpeed unter Windows installieren

Windows ist die GUI-first-Plattform. Das Paket `lightspeed-gui` ist eine eigenständige App, die die Client-Engine und den WinDivert-Treiber bereits enthält, du brauchst also nie einen separaten Client-Download.

---

## Was du herunterladen musst

Hol dir das neueste Release von der [Releases-Seite](https://github.com/ShibbityShwab/lightspeed/releases/latest). Für Windows x86_64 (alle aktuellen Intel- und AMD-PCs) nimm eines von:

| Datei | Nutze sie, wenn |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | Empfohlen. Installiert nach Program Files, legt eine Startmenü-Verknüpfung an und registriert einen Deinstaller. |
| `lightspeed-gui-...-windows-msvc.zip` | Portabel. Irgendwo entpacken und `lightspeed-gui.exe` ausführen. |

Beide enthalten dieselbe GUI plus `WinDivert.dll` und `WinDivert64.sys`.

> **Windows on ARM64 ist noch kein veröffentlichtes Ziel.** Die Release-Builds zielen auf `x86_64-pc-windows-msvc`. Auf einem ARM64-Gerät läuft der x86_64-Build in der Emulationsschicht von Windows, ist aber auf echter ARM64-Hardware ungetestet.

### Windows CLI (nicht unterstützt)

Ein Windows-Kommandozeilen-Build wird ebenfalls als Zip veröffentlicht (`lightspeed-client-...-windows-msvc.zip`). Er ist für Skripting und Headless-Nutzung gedacht, aber **nicht unterstützt**: die GUI ist der empfohlene Weg unter Windows. Wenn du ihn nutzt, starte ihn aus einem erhöhten Terminal (siehe unten).

---

## Installation (MSI)

1. Lade die `.msi` herunter.
2. Doppelklicke sie und folge dem Assistenten. Windows SmartScreen warnt möglicherweise vor einem unbekannten Herausgeber; das Release ist Sigstore-attestiert, du kannst die Attestierung also prüfen, wenn du willst.
3. Starte **LightSpeed** aus dem Startmenü.

## Installation (portables Zip)

1. Lade die `.zip` herunter.
2. Rechtsklick darauf, **Alle extrahieren** wählen und in einen Ordner entpacken, in den du schreiben kannst (zum Beispiel `C:\LightSpeed`). Führe sie nicht direkt aus dem Zip aus.
3. Führe `lightspeed-gui.exe` aus.

---

## Administratorrechte

LightSpeed nutzt den WinDivert-Treiber, um den UDP-Spielverkehr abzufangen. WinDivert braucht **Administrator**-Rechte.

- Die GUI fordert die Erhöhung an, wenn sie den Interceptor starten muss. Bestätige die UAC-Abfrage.
- Wenn du den CLI-Build nutzt, starte ihn aus einem **Administrator**-Terminal (Rechtsklick auf Windows Terminal oder PowerShell, dann **Als Administrator ausführen**).

Ohne Erhöhung kann der Interceptor sich nicht anhängen und du siehst einen Interceptor-Fehler in der Statusansicht.

---

## Erster Start

1. Starte die GUI und bestätige die UAC-Abfrage.
2. Die GUI findet die Community-Relays automatisch über das signierte Register. Keine Proxy-Adresse nötig.
3. Wähle dein Spiel aus der Spielliste.
4. Starte den Interceptor, starte dann dein Spiel und verbinde dich mit einem Server.

Die GUI zeigt Relay-Status, Registrierungszustand und Paketzähler, damit du siehst, wie der Verkehr fließt.

---

## Prüfen, ob es funktioniert

**Prüfe Relay-Erkennung und Registrierung.** Öffne die Statusansicht der GUI. Du solltest die Community-Relays aufgelistet sehen (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) mit einem gesunden Status und einer Registrierungszeile, die zeigt, dass der QUIC-/Auth-Handshake erfolgreich war. Wenn du den CLI-Build nutzt, führe aus:

```powershell
lightspeed-client.exe --probe-proxies
```

Das führt einen Durchlauf aus Erkennung und Sondierung aus und gibt einen sichtbaren Bericht aus, der jedes gefundene Relay und seine Latenz auflistet. Du solltest alle acht Community-Relays sehen.

**Prüfe die Control-Plane-Registrierung.** Mit dem CLI-Build:

```powershell
lightspeed-client.exe --test-control
```

Das verbindet sich mit der QUIC-Control-Plane, registriert eine Sitzung, pingt und trennt wieder, und gibt das Ergebnis jedes Schritts aus. Eine erfolgreiche Registrierung beweist, dass die Control-Plane erreichbar ist und die Authentifizierung funktioniert.

**Prüfe den Paketfluss.** In der GUI sollten die Paketzähler steigen, sobald dein Spiel mit einem Server verbunden ist. Bleibt "Packets Sent" bei 0, hat der Interceptor noch keinen Spielverkehr gesehen; siehe [Troubleshooting](troubleshooting.md).

---

## Logs

Die GUI schreibt ein Trace-Log nach:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Füge diesen Pfad in die Adressleiste des Datei-Explorers ein, um ihn zu öffnen. Hänge ihn an einen Bugreport an, wenn etwas schiefgeht. Was die Log-Zeilen bedeuten, steht in [Troubleshooting](troubleshooting.md).

---

## Deinstallation

- **MSI:** Einstellungen → Apps → Installierte Apps → LightSpeed → Deinstallieren.
- **Zip:** lösche den entpackten Ordner. Die Logdatei unter `%LOCALAPPDATA%\Lightspeed\` bleibt zurück; lösche sie manuell, wenn du eine saubere Entfernung willst.

---

## Nächste Schritte

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
