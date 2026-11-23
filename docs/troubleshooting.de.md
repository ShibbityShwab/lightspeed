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
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

Regeln aus älteren Builds wurden `LightSpeed WinDivert Tunnel <port>` genannt; aktuell
Builds entfernen diejenige für den Port, den sie verwenden, und behalten eine einzige stabile Regel bei. Irgendein
Die verbleibenden Regeln pro Port können sicher gelöscht werden:
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(entspricht dem von `netsh advfirewall firewall show rule name=all` angezeigten Port).

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) oder `EMSGSIZE` (Linux/macOS)

Das Datagramm, das der Client zu senden versuchte, ist größer als die **echte** Pfad-MTU. Die
Die Standard-Tunnelklemme beträgt 1492 Byte (PPPoE-sicher), aber ein VPN, ein DPI-Shim wie z
Zapret oder ein Adapter mit reduzierter MTU können kleiner sein, also ein Paket, das zur Klemme passt
wird immer noch vom Betriebssystem abgelehnt (`Windows error 10040`).

Fix – Senken Sie die Pfad-MTU-Obergrenze des Tunnels in `config.toml`:

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

oder übergeben Sie `--mtu 1400` an der CLI. Aktuelle Builds senken zudem das Budget
Wenn ein Versand automatisch abgelehnt wird, wiederholen Sie das Datagramm einmal mit
Fragmentierung erlaubt, und stattdessen den Betriebssystemfehler im Status aufzeichnen
Meldet das Relais als still.

Sofortige Problemumgehung bei unverändertem Client: Reduzieren Sie die Windows-Adapter
MTU, damit das Spiel kleinere Pakete aussendet, dann starte das Spiel neu:

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
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

### QUIC-Steuerebene schlägt mit `error 120` / `no_application_protocol` fehl

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
In der Zeile `QUIC connection established` wird `alpn=none` angezeigt, und die Registrierung wird fortgesetzt.

**Was der endgültige Fehler bedeutet.** Der Wiederholungsversuch mit leerem Angebot ist ebenfalls fehlgeschlagen, also gibt es solche
genau zwei Möglichkeiten und der Händedruck allein kann sie nicht unterscheiden:

1. **Eine QUIC-abfangende Middlebox auf Ihrem Weg** – ein VPN, Antivirus/Firewall mit
   HTTPS/QUIC-Inspektion oder ISP-Geräte, die nicht die ALPN-Erweiterung tragen
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). Wenn jedes Relais
   Wenn in einem Netzwerk ein Fehler auftritt, in einem anderen jedoch dieselben Relays funktionieren, ist der Pfad der gemeinsame
   Faktor.
2. **Versionsversatz** – das Relay erfordert eine ALPN-Revision, die dieser Client nicht anbietet
   (z. B. ein Relais, das auf `control_alpn = "required"` eingestellt ist und ein Protokoll ausführt
   Revision neuer als dieser Client). Das fällt konstruktionsbedingt lautstark aus, niemals leise
   Downgrade: Ein aktueller Client verhandelt gegen ein Relay auf `lightspeed-control/1`,
   und ein Relay, das etwas erfordert, was dieser Client nicht anbietet, lehnt es mit 120 ab.

**So grenzen Sie es ein:**

- Führen Sie `lightspeed-client --test-control --proxy <relay-ip>` aus, um zu sehen, in welcher Phase das ist
  Relais ist an. Der `alpn=`-Wert auf der verbundenen Leitung und in der endgültigen Zusammenfassung ist der
  ausgehandeltes Anwendungsprotokoll: `lightspeed-control/1` bedeutet, dass das Relais das akzeptiert hat
  versioniertes Angebot, `none` bedeutet, dass nach dem Fallback ein leeres Angebot angenommen wurde (ein älteres).
  Staffel), und ein Lauf, der mit dem Urteil dieses Abschnitts fehlschlägt, ist der oben genannte Fall.
- Wenn jedes Relais in diesem Netzwerk ausfällt, die gleichen Relais aber anderswo funktionieren, liegt der Verdacht nahe
  Middlebox: Deaktivieren Sie das VPN oder die HTTPS/QUIC-Überprüfung des Sicherheitsprodukts und versuchen Sie es erneut.
  Hängen Sie dann eine UDP 4433-Erfassung an #140 an.
- Wenn nur ein Relay ausfällt, vergleichen Sie den ALPN-Modus dieses Relays mit Ihrem Client-Build: dem
  Die Startleitung des Relais und das `control_alpn`-Feld auf seinem `GET /health` melden die
  konfigurierten Modus (siehe [CLI-Referenz](CLI-REFERENCE.md#proxy-configuration)). Wenn Sie
  Wenn Sie dieses Relais betätigen, stellt `control_alpn = "off"` das vorherige Verhalten für ältere Geräte wieder her
  Kunden; Das Operator-Verfahren und die Client-First-Order-Regel sind enthalten
  [Proxy bereitstellen](deploy-proxy.md) und der [ALPN-Rollout-Plan](quic-alpn-rollout.md).

### Das Spiel wird unterbrochen, wenn der Interceptor startet

Der Interceptor erfasst Pakete, bevor das Spiel Antworten empfangen kann, und der Injektionspfad schlägt fehl.

**Windows:**
1. Stellen Sie sicher, dass sich `WinDivert64.sys` und `WinDivert.dll` neben dem `.exe` befinden
2. Trennen Sie sekundäre Netzwerkadapter (virtuelle Docker-, VMware- und Hamachi-Adapter)
3. Stellen Sie eine Verbindung zum Spieleserver her, **bevor** Sie den Interceptor starten

**Linux:**
1. Überprüfen Sie die Nftables-Regeln: `sudo nft list ruleset | grep lightspeed`
2. Wenn die Regeln veraltet sind: `sudo lightspeed --check` zur Diagnose

### „WinDivert-Öffnung fehlgeschlagen“ / `FWP_E_IN_USE` (0x8032000A) unter Windows

Nur die Handles mit einem echten Capture-Filter – das Intercept-Handle (`udp and outbound …`) und das Shadow-Handle (`udp and inbound …`) – besitzen ein WFP-Callout/Filter. Das prozessweite Pin-Handle und das Inject-Handle verwenden den Filter `"false"`, leiten keinen Verkehr um und besitzen kein WFP-Objekt. Ein WFP-Callout/Filter ist daher nicht „eines pro offenem Handle“, und ein gehaltenes Pin ist keine Sperre für den WFP-Zustand: Im Feldprotokoll vom 23.11.2026 ist das Pin geöffnet und das nächste Intercept-Öffnen schlägt weiterhin mit `FWP_E_IN_USE` fehl.

`FWP_E_IN_USE` bedeutet, dass das WFP-Objekt noch referenziert wird (zum Beispiel ein Filter eines früheren Prozesses, den der Kernel nach einem harten Kill noch nicht fertig gelöscht hat), oder dass ein Callout gerade parallel registriert/deregistriert wird. „Kein Prozess verwendet sichtbar WinDivert“ beweist nicht, dass niemand die Referenz hält, und es wird derzeit kein WFP-Zustand erfasst; behandeln Sie den Fehlertext daher nicht als Ursache.

LightSpeed schließt die Intercept-, Inject- und Shadow-Handles auf jedem geordneten Shutdown-Pfad (Strg+C in `--watch`/`--start-interceptor`, **Beenden** in der GUI), entsperrt die Empfangsschleife vor dem Schließen mit `WinDivertShutdown` und wartet auf die Bestätigung der Besitzer-Threads. Diese Determinismus-Zusage gilt für die **Handle-Ownership** der App; das WFP-Teardown im Kernel ist ein separater Pfad, dessen Fehler für den Userspace nicht sichtbar sind.

Wenn es weiterhin auftritt:

1. **Starten Sie einmal erneut.** Der nächste Patch wiederholt `FWP_E_IN_USE` automatisch, solange das Pin gehalten wird (begrenzt, ca. 5 s), und kann das Pin einmal neu aufsetzen; 1.12.5 wiederholt nur, solange **kein** Pin gehalten wird – ein erneuter Versuch bedeutet dort also Stop und erneut Optimieren.
2. **Beenden Sie ordnungsgemäß und warten Sie einen Moment**: Verwenden Sie Strg+C in der CLI oder **Beenden** in der GUI und geben Sie den Handles ein bis zwei Sekunden zum Schließen, bevor Sie neu starten.
3. **Beenden Sie den WinDivert-Dienst** (Administrator; dies ist ein ausdrücklicher Reset, den LightSpeed nicht automatisch ausführt, und er betrifft jede andere WinDivert-basierte App auf dem Rechner – stellen Sie zuerst sicher, dass kein anderer WinDivert-Client läuft):
   ```powershell
   sc stop windivert
   ```
4. **Vollständiges Herunterfahren** ist die letzte Möglichkeit und nur, wenn ein externer WinDivert-Nutzer nicht ausgeschlossen werden kann: Ein Windows-**Neustart** kann die Kernel-Sitzung mit dem veralteten Zustand wiederverwenden, daher ist ein vollständiges **Herunterfahren → Einschalten** die einzige zuverlässige Bereinigung auf Maschinenebene. Es ist nicht die erste Maßnahme, und LightSpeed rät nicht ohne diesen Grund zum Neustart.

WinDivert 2.2 hält seinen Treiber nach dem letzten Handle geladen: Der WFP-Provider und seine 20 Unterschichten bleiben bestehen (1 / 20) und `WinDivert64.sys` bleibt gesperrt, bis der Dienst beendet oder der Rechner neu gestartet wird. Das ändert das Pin nicht, und es ist keine Garantie dafür, dass der Treiber bei einem Stop entladen wird.

> **Tipp:** In v1.2.2 und früher fror ein separater Fehler (Datenebenen-Authentifizierung verwarf alle Pakete, Issue #59) die Verbindung ein und zwang Benutzer, den Client wiederholt zu beenden – das löste die meisten `FWP_E_IN_USE`-Meldungen aus. Dieser Auth-Fehler ist in v1.2.3 behoben.

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

Sobald die Registrierung erfolgreich ist und der Herzschlag zu fließen beginnt, steigt der Zähler. Wenn es auf 0 bleibt, während ein Relais als fehlerfrei anzeigt, führen Sie `lightspeed-client --test-control` aus, um zu isolieren, ob die Steuerungsebene erreichbar ist.

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