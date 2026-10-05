# Häufig gestellte Fragen

---

## Grundlagen

### Ist LightSpeed wirklich kostenlos?

Ja. LightSpeed ist für persönliche, nicht-kommerzielle Nutzung unter der LightSpeed Software License kostenlos. Kommerzielle Nutzung erfordert eine bezahlte Lizenz, siehe [LICENSE](../LICENSE). Du betreibst deinen eigenen Proxy auf einem kleinen VPS (siehe [deployment guide](../infra/README.md)). Es gibt keine Abos, keine Nutzungsgebühren, keine bezahlten Stufen.

### Wird LightSpeed mich bannen?

Nein. LightSpeed nutzt dieselbe Klasse von Netzwerktreibern auf Betriebssystemebene (WinDivert/nftables/pfctl), die auch andere Paket-Capture-Werkzeuge verwenden. Es verändert keine Spieldateien, keinen Speicher und keine Prozesse. Alle großen Anti-Cheat-Systeme (EAC, VAC, BattlEye, Riot Vanguard) erlauben das. Spielserver sehen deine echte IP-Adresse, das ist ein transparenter Tunnel, kein VPN oder Anonymisierer.

### Warum braucht der Interceptor root/Administrator?

Paketabfangen auf Kernel-Ebene erfordert erhöhte Rechte, aus demselben Grund brauchen VPNs und Firewalls sie. Unter Linux nutzt er nftables/iptables. Unter macOS nutzt er pfctl. Unter Windows nutzt er WinDivert (einen signierten Kernel-Treiber). Ohne root kannst du trotzdem den Redirect-Modus nutzen (`--game-server`).

### Welche Plattformen werden unterstützt?

| Plattform | Interceptor | Redirect Mode | GUI |
|----------|-------------|---------------|-----|
| Windows 10/11 | ✅ WinDivert | ✅ | ✅ egui |
| Linux | ✅ nftables/iptables | ✅ | ❌ CLI only |
| macOS | ✅ pfctl | ✅ | ❌ CLI only |
| Linux ARM64 | ✅ | ✅ | ❌ |

---

## Wie es funktioniert

### Wie senkt LightSpeed den Ping tatsächlich?

LightSpeed macht deinen Verkehr **nicht** schneller, Pakete können die Lichtgeschwindigkeit nicht überholen. Was es tut, ist deinen Verkehr *proaktiv auf den schnellsten verfügbaren Pfad zu leiten*, weg von Stau und unnötig langen Umwegen.

Dein ISP schickt Pakete über den Pfad, der für *ihn* am günstigsten ist, oft überlastet oder verschlungen. LightSpeed schickt deine Pakete durch einen Proxy in einem großen Rechenzentrum mit direkten Backbone-Verbindungen zu den Regionen der Spielserver. Ist dieser Pfad kürzer oder weniger überlastet als die Standardroute deines ISP, sinkt dein Ping und wird stabiler. Typische Verbesserung: 10-40 ms.

### Mein Ping ist GESTIEGEN. Warum?

Zwei häufige Gründe:

1. **Falscher Proxy-Standort** - liegt der Proxy weiter vom Spielserver entfernt als dein direkter Pfad, fügt der zusätzliche Hop Latenz hinzu. Das ist die häufigste Ursache. Faustregel: wähle den Proxy, der dem **Spielserver** am nächsten liegt, nicht dir.
2. **Schlecht angebundener Proxy** - nicht alle Rechenzentren sind gleich. Ein Proxy hilft nur, wenn dieses Rechenzentrum nahe an einem großen Internet-Backbone oder Peering-Exchange sitzt. Ein billiger VPS in der "richtigen" Stadt, aber mit überlastetem oder privatem Upstream, kann langsamer sein als deine direkte Route.

LightSpeed kann nur die Route optimieren, die es bekommt. Zeigst du ihm einen schlecht platzierten Proxy, steigt dein Ping, das ist erwartetes Verhalten, kein Bug.

### Welchen Proxy sollte ich wählen?

Den Proxy, der der **Region des Spielservers** am nächsten liegt. Beispiele:
- Du spielst auf US-West-Servern → wähle einen US-West-Proxy
- Du spielst aus Australien auf Singapore-Servern → wähle einen Singapore-Proxy
- Du spielst aus Nordamerika auf EU-Servern → wähle einen Frankfurt-/London-Proxy

### Ein Hinweis zur Routing-Realität (BGP)

Echtes Internet-Routing wird von **BGP** (Border Gateway Protocol) gesteuert, den Verträgen und Richtlinien, mit denen ISPs und Transit-Anbieter Verkehr übergeben. Deine Pakete fliegen nicht geradeaus; sie folgen dem Pfad, den die BGP-Tabellen und Peering-Vereinbarungen festlegen, und Anbieter priorisieren oder deprioritisieren bestimmte Routen aus Kosten- oder Richtliniengründen.

Was das für dich bedeutet:

- Ein Proxy hilft nur, wenn er auf einem *besseren* BGP-Pfad sitzt als die Standardroute deines Heimanschlusses, typischerweise ein Rechenzentrum nahe einem großen Backbone oder Peering-Punkt.
- "Näher auf der Karte" heißt nicht immer "schneller auf der Leitung."
- Routenoptimierungswerkzeuge (auch LightSpeed) schätzen und leiten um, aber der physische Pfad wird letztlich von den Netzwerken dazwischen bestimmt, die weder du noch LightSpeed kontrollieren.

### Wie schnell ist die automatische Erkennung?

Meist 1-3 Sekunden, nachdem du dich mit einem Spielserver verbunden hast. Der Interceptor wartet auf 3 Pakete an dasselbe Ziel innerhalb von 1,5 Sekunden, bevor er einrastet.

### Wie entscheidet LightSpeed, wo Relays hinzukommen?

Der Proxy leitet das **Land** einer IP-Adresse aus einer lokal gespeicherten DB-IP-Lite-Datenbank ab. Das passiert im Speicher, flüchtig, bei der Sitzungserstellung, sowohl für die Quelladresse des Clients als auch für die Zieladresse des Spielservers. Danach zählt er Sitzungen pro `(source_country, destination_country)`-Paar, damit das Netzwerk sieht, welche Regionspaare unterversorgt sind. Eine Zelle wird unterdrückt, bis sie mindestens 3 Sitzungen hat, bevor sie exportiert wird, und die öffentlichen Statistiken enthalten nur vergröberte Regionspaar-Zählungen (zum Beispiel `mena-eu`). Die Zähler enthalten keine rohe IP, und keine rohe IP wird von der Placement-Pipeline exportiert.

---

## FEC (Paketreparatur)

### Was ist FEC?

Forward Error Correction. Der Relay sendet eine kleine Menge redundanter Daten
(bis zu rund 25% bei der Standardblockgröße) zusammen mit deinen Paketen. Geht
eines verloren, kann es aus der Parität rekonstruiert werden, ohne
Neuübertragung, die Wiederherstellung braucht also keinen Round Trip zum
Spielserver.

### Muss ich es aktivieren?

Nein. Die Paketreparatur ist standardmäßig an und passt sich an den gemessenen
Verlust an: eine saubere Leitung hat praktisch keinen Overhead, und die Parität
steigt nur, während tatsächlich Pakete verloren gehen. (Frühere Builds hatten
einen manuellen Schalter, den "Reliability Shield"; seit 1.7 ist immer die
adaptive Richtlinie zuständig.)

---

## Einen Proxy betreiben

### Wie bekomme ich einen Proxy-Node?

Du musst nichts tun. LightSpeed bringt das Community-Relay-Netzwerk als Standard mit: acht von Sponsoren finanzierte Relays (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney), die der Client automatisch über ein signiertes Register findet. Die Registry-URL und der öffentliche Schlüssel des Betreibers sind in den Client einkompiliert, es ist also keine Einrichtung und keine Konfigurationsdatei nötig.

Willst du ein anderes Register nutzen, überschreibe es mit `--registry <url>` oder einem `[registry]`-Block in `lightspeed.toml`. Siehe [Community Relay Network guide](community-network.md).

### Muss ich meinen eigenen Proxy betreiben?

Nein. Das Community-Netzwerk ist der Standard und funktioniert sofort. Selbst-Hosting wird trotzdem voll unterstützt, wenn du ein eigenes dediziertes Relay willst: betreibe einen schlanken Proxy (~500KB RAM) auf einem beliebigen Linux-VPS. Siehe [deployment guide](../infra/README.md).

### Was kostet ein Proxy?

Nichts, wenn du das Community-Netzwerk nutzt. Wenn du selbst hostest, kostet ein kleiner VPS ein paar Dollar im Monat, und die Proxy-Binary braucht ~500KB RAM, selbst die kleinste Instanz reicht also dicke. LightSpeed selbst hat keine Gebühren.

### Kann ich meinen Proxy mit Freunden teilen?

Ja. Der Proxy unterstützt mehrere gleichzeitige Sitzungen mit Ratenbegrenzung pro Client und Authentifizierung. Konfiguriere Tokens in `proxy.toml`.

---

## Fehlerbehebung

### "No game traffic seen"

- Stelle sicher, dass dein Spiel tatsächlich mit einem Server verbunden ist (nicht nur im Hauptmenü)
- Prüfe, ob du das richtige Spiel ausgewählt hast (`--game`-Flag)
- Versuche `--scan-processes`, um laufende Spielprozesse aufzulisten
- Nutzt dein Server einen Nicht-Standard-Port, verwende den manuellen Servermodus (`--game-server`)

### "Interceptor not available"

- Linux: stelle sicher, dass du als root läufst und nftables/iptables installiert ist
- macOS: pfctl ist eingebaut, braucht aber root
- Windows: stelle sicher, dass `WinDivert64.sys` und `WinDivert.dll` neben der `.exe` liegen

### Pakete gesendet, aber nicht zugestellt

Deine Pakete erreichen den Proxy, aber die Antworten erreichen dein Spiel nicht. Meist ein Firewall-Problem. LightSpeed versucht, Firewall-Regeln automatisch hinzuzufügen. Schlägt das fehl, füge manuell eine eingehende UDP-Regel für `lightspeed` oder `lightspeed-gui.exe` hinzu.

---

## Datenschutz

### Liest LightSpeed meinen Spielverkehr?

LightSpeed sieht UDP-Paketheader (Quell-/Ziel-IP, Port, Größe), um sie weiterzuleiten. Spielinhalte (Spielerpositionen, Chat usw.) sind durch das Protokoll des Spiels selbst verschlüsselt und werden nicht entschlüsselt oder protokolliert. Siehe die vollständige [Privacy Policy](privacy.md).

### Gibt es Telemetrie?

Telemetrie ist seit v1.6.5 **standardmäßig an**. Sie sendet anonymisierte aggregierte Metriken (RTT-Perzentile, Jitter, FEC-Statistiken und die direkten/weitergeleiteten/gesparten Latenzzahlen) an den `/telemetry`-Endpunkt des Relays, mit dem du verbunden bist (ein Community- oder Sponsor-Relay oder dein eigenes, wenn du selbst hostest). Es werden keine IP-Adressen, Tokens, Identifikatoren oder Daten von Spielkonten erfasst. Eine Zelle wird unterdrückt, bis sie mindestens 3 Reports hat; diese Untergrenze zählt Reports, nicht verschiedene Personen, sie garantiert also nicht, dass 3 verschiedene Personen beigetragen haben. Reports werden per Klartext-HTTP an Port 8080 gesendet, und der Endpunkt ist unauthentifiziert. Schalte sie jederzeit ab mit `--no-telemetry`, `telemetry = false` unter `[general]` in `lightspeed.toml` oder dem Kontrollkästchen **"Share anonymous latency stats"** in der GUI. Siehe [Privacy Policy](privacy.md) und das [Data Dictionary](data-dictionary.md).

### Speichert der Proxy meine IP-Adresse?

Nein. Der Proxy verarbeitet deine Quell-IP und die Ziel-IP des Spielservers flüchtig im Speicher, um Pakete weiterzuleiten und ein Land für die Placement-Analyse abzuleiten. Die Adresse selbst wird nicht gespeichert. Behalten wird ein aggregierter Sitzungszähler pro `(source_country, destination_country)`-Paar, mit einer Untergrenze von 3 Sitzungen pro Zelle, damit kleine Zellen unterdrückt werden, und die öffentlichen Statistiken enthalten nur vergröberte Regionspaar-Zählungen. Die Placement-Zähler enthalten keine rohe IP, und keine rohe IP wird von der Placement-Pipeline exportiert; der Telemetrievertrag des Clients bleibt unverändert. Beachte, dass Zugriffslogs des Relays Client-IPs enthalten können; siehe [What does the proxy log?](#what-does-the-proxy-log).

### Was protokolliert der Proxy?

Betriebslogs, die nach stdout geschrieben werden (konfigurierbar über `RUST_LOG`), können Client-IPs, Sitzungsstart- und -endzeiten und weitergeleitete Bytes enthalten. Client-IPs erscheinen dort nur für Ratenbegrenzung und Abuse-Erkennung. Diese Logs liegen auf dem Relay, das die Sitzung bedient hat (ein Community- oder Sponsor-Relay oder dein eigenes, wenn du selbst hostest), werden nicht exportiert und nicht mit den Placement-Zählern verknüpft. Die Aufbewahrungsdauer richtet sich nach der Logging-Konfiguration dieses Relays. Siehe [Privacy Policy](privacy.md).

---

## Sonstiges

### Kann ich LightSpeed mit einem VPN nutzen?

Generell nein, beide versuchen, Netzwerkverkehr abzufangen, und geraten in Konflikt. Deaktiviere dein VPN, bevor du LightSpeed nutzt.

### Funktioniert LightSpeed mit Cloudflare WARP?

Ja. Nutze `--warp`, um WARP für die Proxy-Strecke der Verbindung zu aktivieren. WARP kann 5-10 ms beim lokalen ISP-Routing sparen. Kombiniere es mit einem Proxy für maximalen Nutzen.

### Wo melde ich Bugs?

[Eröffne ein Issue auf GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Gib dein OS, dein Spiel und die Logausgabe an (führe mit `RUST_LOG=debug` für ausführliche Logs aus).
