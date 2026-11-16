# Fehlerbehebung

> [!WARNING]
> Maschinell unterstützte Übersetzung, nicht von Muttersprachlern geprüft. Maßgeblich ist die [deutsche Fassung](troubleshooting.md).

---

## Schnelle Diagnose

Führen Sie zuerst die integrierte Umgebungsprüfung aus – sie erkennt die meisten Probleme:

```bash
lightspeed --check
```

Dies überprüft: Interceptor-Verfügbarkeit, Paketfilter-Tools, Spielprofilauflösung und Proxy-Konnektivität.

---

## Häufige Probleme

### „Interceptor nicht verfügbar“

**CLI:** Ihr Betriebssystem verfügt nicht über die erforderlichen Paketfilterungstools oder Ihnen fehlen Berechtigungen.

| Betriebssystem | Erforderlich | So beheben Sie |
|----|----------|------------|
| Linux | nftables oder iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (integriert) + root | `sudo lightspeed ...` |
| Windows | WinDivert-Treiber + Administrator | Klicken Sie mit der rechten Maustaste → Als Administrator ausführen |

Überprüfen Sie mit:
```bash
lightspeed --check
```

### „Kein Spielverkehr gesehen“ / Gesendete Pakete bleiben auf 0

**CLI:** Der Interceptor kann keine Spielpakete im erwarteten Portbereich finden.

1. Stellen Sie sicher, dass Ihr Spiel **mit einem Server verbunden** ist (nicht nur mit dem Hauptmenü oder der Lobby)
2. Überprüfen Sie das Spiel mit `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Probieren Sie ein anderes Spielprofil aus oder verwenden Sie den manuellen Servermodus

**GUI (Windows):** Warten Sie 15 Sekunden. Wenn das gelbe Banner „⚠ Kein Spielverkehr gesehen“ erscheint:
1. Öffnen Sie PowerShell mit erhöhten Rechten:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Verwenden Sie den unter **Erweitert → Server manuell festlegen** angezeigten Port.

### „🎯 Finden Sie Ihren Spieleserver…“ löst sich nie auf

Der Detektor hat innerhalb von 1,5 Sekunden keine 3 Pakete an dasselbe Ziel erkannt.

1. Stellen Sie sicher, dass Sie mit einem Spieleserver verbunden sind (bewegen Sie Ihren Charakter, um Datenverkehr zu generieren).
2. Wenn nach 15 Sekunden immer noch keine Pakete erkannt werden, befindet sich Ihr Server an einem nicht standardmäßigen Port – verwenden Sie den manuellen Servermodus
3. Stoppen Sie den Interceptor und starten Sie ihn neu, nachdem Sie eine Verbindung zum Server hergestellt haben

### Gesendete Pakete steigen, Zugestellte Pakete = 0

Pakete erreichen den Proxy, aber die Antworten erreichen Ihr Spiel nicht. Normalerweise ein Firewall-Problem.

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

### Proxy-Zustandsprüfung schlägt fehl

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Falls nicht erreichbar:
- Überprüfen Sie, ob der Proxy ausgeführt wird: `systemctl status lightspeed-proxy`
- Überprüfen Sie, ob die Firewall UDP 4434 und TCP 8080 zulässt
- Überprüfen Sie die Proxy-Protokolle: `journalctl -u lightspeed-proxy --tail 50`

### QUIC-Steuerungsebene schlägt mit `error 120` / `no_application_protocol` fehl

Der Client konnte den QUIC-Steuerungsebenen-Handshake mit einem Relay nicht abschließen. Wenn die
Da der einmalige Wiederholungsversuch mit leerem Angebot ebenfalls fehlgeschlagen ist, endet das Protokoll mit diesem Urteil (zitiert
wörtlich vom Kunden; `{err}` ist der zugrunde liegende QUIC-Fehler und `{addr}` der
Antwortadresse):

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**Was der Client bereits getan hat.** Aktuelle Clients bieten immer den versionierten ALPN-Namen an
`lightspeed-control/1`. Wenn das Relay mit der TLS-Warnung 120 antwortet, versucht der Client es erneut
**Einmal** mit einem leeren ALPN-Angebot und merkt sich dann für den Rest die Antwort dieser Staffel
des Prozesses, sodass im normalen Client-Betrieb der zum Scheitern verurteilte zusätzliche Handshake auftritt
Meistens einmal pro Staffel und Start. Gegen ein Relay ist ein erfolgreicher Fallback normal
wurde nicht aktualisiert (der Relaisstandard ist `[network] control_alpn = "off"`): die
In der Zeile `QUIC connection established` wird `alpn=none` angezeigt und die Registrierung wird fortgesetzt.

**Was der endgültige Fehler bedeutet.** Der Wiederholungsversuch mit leerem Angebot ist ebenfalls fehlgeschlagen, also gibt es solche
genau zwei Möglichkeiten und der Händedruck allein kann sie nicht unterscheiden:

1. **Eine QUIC-abfangende Middlebox auf Ihrem Weg** – ein VPN, Antivirus/Firewall mit
   HTTPS/QUIC-Inspektion oder ISP-Geräte, die nicht die ALPN-Erweiterung tragen
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). Wenn jedes Relais
   Wenn in einem Netzwerk ein Fehler auftritt, in einem anderen jedoch dieselben Relays funktionieren, ist der Pfad der gemeinsame
   Faktor.
2. **Versionsversatz** – das Relay erfordert eine ALPN-Revision, die dieser Client nicht anbietet
   (zum Beispiel ein Relais, das auf `control_alpn = "required"` eingestellt ist und ein Protokoll ausführt
   Revision neuer als dieser Client). Das fällt konstruktionsbedingt lautstark aus, niemals leise
   Downgrade: ein aktueller Client gegen ein Relay auf `lightspeed-control/1` verhandelt,
   und ein Relay, das etwas erfordert, was dieser Client nicht anbietet, lehnt es mit 120 ab.

**So grenzen Sie es ein:**

- Führen Sie `lightspeed-client --test-control --proxy <relay-ip>` aus, um zu sehen, in welcher Phase das ist
  Relais ist an. Der `alpn=`-Wert auf der verbundenen Linie und in der endgültigen Zusammenfassung ist der
  Ausgehandeltes Anwendungsprotokoll: `lightspeed-control/1` bedeutet, dass das Relay das akzeptiert hat
  versioniertes Angebot, `none` bedeutet, dass nach dem Fallback ein leeres Angebot angenommen wurde (ein älteres
  Staffel), und ein Lauf, der mit dem Urteil dieses Abschnitts fehlschlägt, ist der oben genannte Fall.
- Wenn jedes Relais in diesem Netzwerk ausfällt, die gleichen Relais aber anderswo funktionieren, liegt der Verdacht nahe
  Middlebox: Deaktivieren Sie das VPN oder die HTTPS/QUIC-Überprüfung des Sicherheitsprodukts und versuchen Sie es erneut.
  Hängen Sie dann eine UDP 4433-Erfassung an #140 an.
- Wenn nur ein Relay ausfällt, vergleichen Sie den ALPN-Modus dieses Relays mit Ihrem Client-Build: dem
  Die Startzeile des Relais und das Feld `control_alpn` auf seinem `GET /health` melden das
  konfigurierten Modus (siehe [CLI-Referenz](CLI-REFERENCE.md#proxy-configuration)). Wenn Sie
  Betätigen Sie dieses Relais, `control_alpn = "off"` stellt das vorherige Verhalten für ältere wieder her
  Kunden; Das Operator-Verfahren und die Client-First-Order-Regel sind enthalten
  [Proxy bereitstellen](deploy-proxy.md) und der [ALPN-Rollout-Plan](quic-alpn-rollout.md).

### Das Spiel wird unterbrochen, wenn der Interceptor startet

Der Interceptor erfasst Pakete, bevor das Spiel Antworten empfangen kann, und der Injektionspfad schlägt fehl.

**Windows:**
1. Stellen Sie sicher, dass `WinDivert64.sys` und `WinDivert.dll` neben `.exe` liegen.
2. Trennen Sie sekundäre Netzwerkadapter (virtuelle Docker-, VMware- und Hamachi-Adapter)
3. Stellen Sie eine Verbindung zum Spieleserver her, **bevor** Sie den Interceptor starten

**Linux:**
1. Nftables-Regeln prüfen: `sudo nft list ruleset | grep lightspeed`
2. Wenn Regeln veraltet sind: `sudo lightspeed --check` zur Diagnose

### „WinDivert-Öffnung fehlgeschlagen“ / `FWP_E_IN_USE` (0x8032000A) unter Windows

WinDivert registriert einen WFP-Callout/Filter für jedes offene Handle. Wenn ein Handle nie geschlossen wird, bleibt dieser Filterstatus bestehen und der nächste `WinDivertOpen` schlägt mit `FWP_E_IN_USE` (0x8032000A) fehl, obwohl kein Prozess sichtbar WinDivert verwendet.

Aktuelle LightSpeed-Builds schließen sowohl die Capture- als auch die Inject-Handles auf jedem ordnungsgemäßen Shutdown-Pfad, einschließlich Strg+C in `--watch` und `--start-interceptor` und **Quit** in der GUI, und die Empfangsschleife wird vor dem Schließen mit `WinDivertShutdown` entsperrt, sodass das Teardown deterministisch ist. Ein Hard Kill (`taskkill /f`, ein Absturz oder das Schließen des Konsolenfensters) kann immer noch dazu führen, dass der WinDivert 2.2.x-Treiber veraltet ist. Das ist eine Einschränkung des Upstream-Treibers (basil00/WinDivert#294, #406), die der Userspace nicht löschen kann, sobald der Prozess beendet ist.

Wenn Sie es immer noch treffen:

1. **Beenden Sie ordnungsgemäß und warten Sie einen Moment**: Verwenden Sie die CLI-Strg+C-Taste oder **Beenden** der GUI und geben Sie den Handles dann ein oder zwei Sekunden Zeit, um sie zu schließen, bevor Sie sie neu starten.
2. **Beenden Sie den WinDivert-Dienst** (vermeidet in manchen Fällen einen Neustart; beachten Sie, dass der Treiber mit allen anderen WinDivert-basierten Apps auf dem Computer gemeinsam genutzt wird):
   ```powershell
   sc stop windivert
   ```
3. **Vollständiges Herunterfahren, kein Neustart**: Windows „Neustart“ kann die Kernel-Sitzung wiederverwenden, die den veralteten Zustand enthält; ein vollständiges **Herunterfahren → Einschalten** löscht es.

> **Tipp:** In Version 1.2.2 und früher fror ein separater Fehler (Authentifizierung auf Datenebene lehnte alle Pakete ab, Problem Nr. 59) die Verbindung ein und zwang Benutzer, den Client wiederholt zu beenden, was die meisten `FWP_E_IN_USE` Berichte auslöste. Dieser Authentifizierungsfehler wurde in Version 1.2.3 behoben.

---

## Probleme mit der Windows-GUI

Diese gelten für die `lightspeed-gui`-App unter Windows.

### Beenden hat nichts bewirkt und einen Zombie-Prozess hinterlassen

Bei Versionen vor Version 1.4.2 konnte es passieren, dass der Prozess im Hintergrund weiterlief (ein Zombie), wenn Sie im Taskleistenmenü **Beenden** auswählten. Das Fenster wurde geschlossen, die Engine lief jedoch weiter und ein späterer Start verhielt sich seltsam. v1.4.2 behebt dieses Problem: „Quit now“ beendet den Prozess sauber.

Wenn Sie einen älteren Build verwenden und der Prozess hängen bleibt, beenden Sie ihn manuell:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Dann aktualisieren Sie auf v1.4.2 oder höher.

### Eine zweite Instanz wird geöffnet, anstatt die erste zu fokussieren

Bei Versionen vor v1.4.2 wurden beim zweimaligen Starten der GUI ein zweites Fenster und eine zweite Engine gestapelt. Die GUI verfügt jetzt über einen Einzelinstanzschutz: Beim zweiten Start erscheint eine kurze Meldung „LightSpeed ​​läuft bereits“ und das Programm wird beendet, anstatt eine andere Engine zu starten. Um trotzdem eine zweite Instanz zu erzwingen (zur Diagnose), übergeben Sie `--force` oder legen Sie `LIGHTSPEED_GUI_FORCE=1` fest.

### Keine Relais entdeckt

Die GUI erkennt die Community-Relays über die signierte Registrierung. Wenn die Relaisliste leer bleibt:

1. Stellen Sie sicher, dass Sie über einen Internetzugang verfügen und dass ausgehende HTTPS-Verbindungen zur Registrierung nicht durch eine Firewall oder ein VPN blockiert werden.
2. Überprüfen Sie das GUI-Protokoll (siehe unten) auf einen Registrierungsabruf- oder Signaturüberprüfungsfehler.
3. Führen Sie im CLI-Build `lightspeed-client --probe-proxies` aus, um den Erkennungs- und Testbericht direkt anzuzeigen. Wenn die CLI ebenfalls nichts findet, ist das Problem netzwerkseitig und nicht GUI-spezifisch.
4. Starten Sie die GUI neu, nachdem Sie die Konnektivität repariert haben. Die Erkennung wird beim Start ausgeführt.

### So finden und öffnen Sie das GUI-Protokoll

Die GUI schreibt ihr Trace-Protokoll an:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Fügen Sie diesen Pfad in die Adressleiste des Datei-Explorers ein, um den Ordner zu öffnen, und öffnen Sie dann `gui-trace.log` in einem beliebigen Texteditor. Hängen Sie es an einen Fehlerbericht an.

### „Heartbeat 0 in“ im Protokoll

Eine Zeile wie `Heartbeat 0 in` bedeutet, dass die Engine im aktuellen Fenster keine Keepalive-Heartbeats gesendet hat. In der Praxis wird es angezeigt, wenn der Client noch keine funktionierende Verbindung zur Steuerungsebene hergestellt hat und daher keine Heartbeats ausgegeben wurden. Häufige Ursachen:

- Der Client hat sich noch nicht bei einem Relay registriert (überprüfen Sie die Registrierungszeile in der Statusansicht).
- Das ausgewählte Relais ist nicht erreichbar.
- Der Interceptor wurde nicht gestartet, daher ist keine Sitzung aktiv.

Sobald die Registrierung erfolgreich ist und der Herzschlag zu fließen beginnt, steigt der Zähler. Wenn es bei 0 bleibt, während ein Relais als fehlerfrei anzeigt, führen Sie `lightspeed-client --test-control` aus, um festzustellen, ob die Steuerungsebene erreichbar ist.

---

## Protokolle für Fehlerberichte

Mit Debug-Protokollierung ausführen, um detaillierte Diagnosen zu erfassen:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Hängen Sie die Protokolldatei an Ihr [GitHub-Problem](https://github.com/ShibbityShwab/lightspeed/issues) an.

---

## Immer noch nicht weitergekommen?

- [FAQ](faq.md) – häufige Fragen
- [GitHub-Probleme](https://github.com/ShibbityShwab/lightspeed/issues) – Durchsuchen Sie vorhandene Berichte
- Öffnen Sie ein neues Problem mit Ihrem Betriebssystem, Spiel und der Protokollausgabe