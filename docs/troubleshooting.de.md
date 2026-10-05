# Fehlerbehebung

---

## Schnelldiagnose

Führe zuerst die eingebaute Umgebungsprüfung aus, sie fängt die meisten Probleme ab:

```bash
lightspeed --check
```

Das prüft: Interceptor-Verfügbarkeit, Paketfilter-Werkzeuge, Spielprofil-Auflösung und Proxy-Verbindung.

---

## Häufige Probleme

### "Interceptor not available"

**CLI:** Deinem Betriebssystem fehlen die nötigen Paketfilter-Werkzeuge oder dir fehlen die Rechte.

| OS | Erforderlich | So behebst du es |
|----|----------|------------|
| Linux | nftables oder iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (eingebaut) + root | `sudo lightspeed ...` |
| Windows | WinDivert-Treiber + Administrator | Rechtsklick → Als Administrator ausführen |

Prüfe mit:
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent bleibt bei 0

**CLI:** Der Interceptor findet keine Spielpakete im erwarteten Portbereich.

1. Stelle sicher, dass dein Spiel **mit einem Server verbunden** ist (nicht nur im Hauptmenü oder in der Lobby)
2. Prüfe das Spiel mit `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Probiere ein anderes Spielprofil oder nutze den manuellen Servermodus

**GUI (Windows):** Warte 15 Sekunden. Erscheint das bernsteinfarbene Banner "⚠ No game traffic seen":
1. Öffne eine erhöhte PowerShell:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Nutze den angezeigten Port in **Advanced → set server manually**

### "🎯 Finding your game server…" wird nie aufgelöst

Der Detektor hat keine 3 Pakete an dasselbe Ziel innerhalb von 1,5 Sekunden gesehen.

1. Stelle sicher, dass du mit einem Spielserver verbunden bist (bewege deine Spielfigur, um Verkehr zu erzeugen)
2. Werden Pakete nach 15 Sekunden immer noch nicht erkannt, liegt dein Server auf einem Nicht-Standard-Port, nutze den manuellen Servermodus
3. Stoppe und starte den Interceptor neu, nachdem du dich mit dem Server verbunden hast

### Packets Sent steigt, Packets Delivered = 0

Pakete erreichen den Proxy, aber die Antworten erreichen dein Spiel nicht. Meist ein Firewall-Problem.

**Linux:**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS:**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Windows:**
```powershell
# Check if the firewall rule exists
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

### Proxy-Healthcheck schlägt fehl

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Ist er nicht erreichbar:
- Prüfe, ob der Proxy läuft: `systemctl status lightspeed-proxy`
- Prüfe, ob die Firewall UDP 4434 und TCP 8080 erlaubt
- Prüfe die Proxy-Logs: `journalctl -u lightspeed-proxy --tail 50`

### Spiel trennt die Verbindung, wenn der Interceptor startet

Der Interceptor greift Pakete ab, bevor das Spiel Antworten empfangen kann, und der Inject-Pfad schlägt fehl.

**Windows:**
1. Prüfe, ob `WinDivert64.sys` und `WinDivert.dll` neben der `.exe` liegen
2. Trenne sekundäre Netzwerkadapter (Docker-, VMware-, Hamachi-Virtual-Adapter)
3. Verbinde dich mit dem Spielserver, **bevor** du den Interceptor startest

**Linux:**
1. Prüfe die nftables-Regeln: `sudo nft list ruleset | grep lightspeed`
2. Sind die Regeln veraltet: `sudo lightspeed --check` zur Diagnose

### "WinDivert open failed" / `FWP_E_IN_USE` (0x8032000A) unter Windows

WinDivert registriert für jedes geöffnete Handle ein WFP-Callout/Filter. Wird ein Handle nie geschlossen, bleibt dieser Filterzustand bestehen, und das nächste `WinDivertOpen` schlägt mit `FWP_E_IN_USE` (0x8032000A) fehl, obwohl sichtbar kein Prozess WinDivert nutzt.

Neuere LightSpeed-Builds schließen sowohl das Capture- als auch das Inject-Handle auf jedem ordentlichen Shutdown-Pfad, einschließlich Strg+C in `--watch` und `--start-interceptor` und **Quit** in der GUI, und die Empfangsschleife wird mit `WinDivertShutdown` vor dem Schließen entblockt, damit der Abbau deterministisch ist. Ein hartes Beenden (`taskkill /f`, ein Absturz oder das Schließen des Konsolenfensters) kann den WinDivert-2.2.x-Treiber trotzdem mit veraltetem Zustand zurücklassen; das ist eine Einschränkung des Upstream-Treibers (basil00/WinDivert#294, #406), die der Userspace nicht mehr bereinigen kann, sobald der Prozess weg ist.

Wenn du trotzdem darauf stößt:

1. **Beende sauber und warte einen Moment**: nutze Strg+C im CLI oder **Quit** in der GUI, gib den Handles dann ein oder zwei Sekunden zum Schließen, bevor du neu startest.
2. **Stoppe den WinDivert-Dienst** (vermeidet in manchen Fällen einen Neustart; beachte, dass der Treiber mit jeder anderen WinDivert-basierten App auf der Maschine geteilt wird):
   ```powershell
   sc stop windivert
   ```
3. **Vollständiges Herunterfahren, kein Neustart**: Windows "Neu starten" kann die Kernel-Sitzung wiederverwenden, die den veralteten Zustand hält; ein vollständiges **Herunterfahren → Einschalten** räumt ihn auf.

> **Tipp:** In v1.2.2 und früher fror ein separater Bug (Data-Plane-Auth, die alle Pakete ablehnte, Issue #59) die Verbindung ein und zwang Nutzer, den Client wiederholt zu beenden, was die meisten `FWP_E_IN_USE`-Meldungen ausgelöst hat. Dieser Auth-Bug ist in v1.2.3 behoben.

---

## Windows-GUI-Probleme

Diese betreffen die App `lightspeed-gui` unter Windows.

### Quit tat nichts und hinterließ einen Zombie-Prozess

In Versionen vor v1.4.2 konnte die Wahl von **Quit** im Tray-Menü den Prozess im Hintergrund weiterlaufen lassen (einen Zombie), das Fenster schloss sich also, aber die Engine lief weiter und ein späterer Start verhielt sich seltsam. v1.4.2 behebt das: Quit beendet den Prozess jetzt sauber.

Bist du auf einem älteren Build und hängt der Prozess, beende ihn manuell:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Aktualisiere dann auf v1.4.2 oder neuer.

### Eine zweite Instanz öffnet sich, statt die erste zu fokussieren

In Versionen vor v1.4.2 stapelte ein zweimaliger Start der GUI ein zweites Fenster und eine zweite Engine. Die GUI hält jetzt einen Single-Instance-Guard: ein zweiter Start zeigt einen kurzen Hinweis "LightSpeed is already running" und beendet sich, statt eine weitere Engine zu starten. Willst du trotzdem eine zweite Instanz erzwingen (für Diagnose), übergib `--force` oder setze `LIGHTSPEED_GUI_FORCE=1`.

### Keine Relays gefunden

Die GUI findet die Community-Relays über das signierte Register. Bleibt die Relay-Liste leer:

1. Prüfe, ob du Internetzugang hast und ob eine Firewall oder ein VPN ausgehendes HTTPS zum Register blockiert.
2. Prüfe das GUI-Log (siehe unten) auf einen Registry-Fetch- oder Signaturprüfungsfehler.
3. Führe im CLI-Build `lightspeed-client --probe-proxies` aus, um den Erkennungs- und Sondierungsbericht direkt zu sehen. Findet auch das CLI nichts, liegt das Problem netzwerkseitig, nicht an der GUI.
4. Starte die GUI nach der Behebung der Verbindung neu; die Erkennung läuft beim Start.

### Wie du das GUI-Log findest und öffnest

Die GUI schreibt ihr Trace-Log nach:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Füge diesen Pfad in die Adressleiste des Datei-Explorers ein, um den Ordner zu öffnen, öffne dann `gui-trace.log` in einem beliebigen Texteditor. Hänge es an einen Bugreport an.

### "Heartbeat 0 in" im Log

Eine Zeile wie `Heartbeat 0 in` bedeutet, dass die Engine im aktuellen Fenster null Keepalive-Heartbeats gesendet hat. Praktisch taucht das auf, wenn der Client noch keine funktionierende Control-Plane-Verbindung aufgebaut hat, es sind also keine Heartbeats rausgegangen. Häufige Ursachen:

- Der Client hat sich noch bei keinem Relay registriert (prüfe die Registrierungszeile in der Statusansicht).
- Das gewählte Relay ist nicht erreichbar.
- Der Interceptor wurde nicht gestartet, es ist also keine Sitzung aktiv.

Sobald die Registrierung klappt und Heartbeats fließen, steigt der Zähler. Bleibt er bei 0, während ein Relay gesund erscheint, führe `lightspeed-client --test-control` aus, um einzugrenzen, ob die Control-Plane erreichbar ist.

---

## Logs für Bugreports

Führe mit Debug-Logging aus, um detaillierte Diagnosen zu erfassen:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Hänge die Logdatei an dein [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues) an.

---

## Immer noch festgefahren?

- [FAQ](faq.md) - häufige Fragen
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) - durchsuche bestehende Meldungen
- Eröffne ein neues Issue mit deinem OS, deinem Spiel und der Logausgabe
