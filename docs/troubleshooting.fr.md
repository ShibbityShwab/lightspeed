# Dépannage

> [!WARNING]
> Traduction assistée par machine, non relue par un locuteur natif. La [version anglaise](troubleshooting.md) fait foi.

---

## Diagnostic rapide

Exécutez d'abord la vérification de l'environnement intégré : elle détecte la plupart des problèmes :

```bash
lightspeed --check
```

Cela vérifie : la disponibilité des intercepteurs, les outils de filtrage de paquets, la résolution du profil de jeu et la connectivité proxy.

---

## Problèmes courants

### "Intercepteur non disponible"

**CLI :** Votre système d'exploitation ne dispose pas des outils de filtrage de paquets requis ou vous manquez de privilèges.

| Système d'exploitation | Obligatoire | Comment réparer |
|----|----------|------------|
| Linux | nftables ou iptables + racine | `sudo lightspeed ...` |
| macOS | pfctl (intégré) + root | `sudo lightspeed ...` |
| Fenêtres | Pilote WinDivert + Administrateur | Cliquez avec le bouton droit → Exécuter en tant qu'administrateur |

Vérifiez auprès de :
```bash
lightspeed --check
```

### "Aucun trafic de jeu vu" / Les paquets envoyés restent à 0

**CLI :** L'intercepteur ne trouve pas les paquets de jeu dans la plage de ports attendue.

1. Assurez-vous que votre jeu est **connecté à un serveur** (pas seulement au menu principal ou au lobby)
2. Vérifiez le jeu avec `--scan-processes` :
   ```bash
   lightspeed --scan-processes
   ```
3. Essayez un autre profil de jeu ou utilisez le mode serveur manuel

**GUI (Windows) :** Attendez 15 secondes. Si la bannière orange « ⚠ Aucun trafic de jeu vu » apparaît :
1. Ouvrez PowerShell élevé :
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Utilisez le port indiqué dans **Avancé → définir le serveur manuellement**

### "🎯 Trouver votre serveur de jeu…" ne se résout jamais

Le détecteur n'a pas vu 3 paquets vers la même destination en 1,5 seconde.

1. Assurez-vous d'être connecté à un serveur de jeu (déplacez votre personnage pour générer du trafic)
2. Si les paquets ne sont toujours pas détectés après 15 secondes, votre serveur est sur un port non standard – utilisez le mode serveur manuel
3. Arrêtez et redémarrez l'intercepteur après vous être connecté au serveur

### Paquets envoyés grimpant, Paquets livrés = 0

Les paquets atteignent le proxy mais les réponses n'atteignent pas votre jeu. Il s'agit généralement d'un problème de pare-feu.

**Linux :**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS :**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Fenêtres :**
```powershell
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

Les règles des anciennes versions étaient nommées `LightSpeed WinDivert Tunnel <port>` ; courant
les builds suppriment celui du port qu’ils utilisent et conservent une seule règle stable. N'importe lequel
Les règles restantes par port peuvent être supprimées en toute sécurité :
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(correspond au port indiqué par `netsh advfirewall firewall show rule name=all`).

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) ou `EMSGSIZE` (Linux/macOS)

Le datagramme que le client a essayé d'envoyer est plus grand que le MTU du chemin **réel**. Le
la limite de tunnel par défaut est de 1492 octets (PPPoE-safe), mais un VPN, une cale DPI telle que
Zapret, ou un adaptateur à MTU réduit, peut être plus petit, donc un paquet qui s'adapte à la pince
est toujours rejeté par le système d'exploitation (`Windows error 10040`).

Correctif : abaissez le plafond du chemin-MTU du tunnel, dans `config.toml` :

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

ou transmettez `--mtu 1400` sur la CLI. Les versions actuelles réduisent également le budget
automatiquement lorsqu'un envoi est rejeté, réessayez ce datagramme une fois avec
fragmentation autorisée et enregistrez l'erreur du système d'exploitation dans l'état au lieu de
signalant le relais comme silencieux.

Solution de contournement immédiate alors que le client est inchangé : réduisez la valeur de l'adaptateur Windows.
MTU pour que le jeu émette des paquets plus petits, puis redémarrez le jeu :

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### La vérification de l'état du proxy échoue

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Si inaccessible :
- Vérifiez que le proxy est en cours d'exécution : `systemctl status lightspeed-proxy`
- Vérifiez que le pare-feu autorise UDP 4434 et TCP 8080
- Vérifiez les journaux du proxy : `journalctl -u lightspeed-proxy --tail 50`

### Le plan de contrôle QUIC échoue avec `error 120` / `no_application_protocol`

Le client n'a pas pu terminer la négociation du plan de contrôle QUIC avec un relais. Quand le
La nouvelle tentative d'offre vide unique a également échoué, le journal se termine par ce verdict (cité
verbatim du client ; `{err}` est l'erreur QUIC sous-jacente et `{addr}` l'erreur QUIC sous-jacente.
adresse du répondeur) :

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**Ce que le client a déjà fait.** Les clients actuels proposent toujours le nom ALPN versionné
`lightspeed-control/1`. Si le relais répond avec l'alerte TLS 120, le client réessaye
**une fois** avec une offre ALPN vide, puis se souvient de la réponse de ce relais pour le reste
du processus, donc dans le fonctionnement normal du client, la poignée de main supplémentaire vouée à l'échec se produit à
au maximum une fois par relais et par lancement. Un repli réussi est normal contre un relais qui
n'a pas été mis à niveau (le relais par défaut est `[network] control_alpn = "off"`) : le
La ligne `QUIC connection established` affiche `alpn=none` et l'enregistrement se poursuit.

**Ce que signifie l'échec final.** La nouvelle tentative d'offre vide a également échoué, il y a donc
exactement deux possibilités et la poignée de main seule ne peut pas les distinguer :

1. **Un middlebox interceptant QUIC sur votre chemin** - un VPN, un antivirus/pare-feu avec
   Inspection HTTPS/QUIC ou équipement FAI ne portant pas l'extension ALPN
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). Si chaque relais
   échoue sur un réseau mais les mêmes relais fonctionnent sur un autre, le chemin est le commun
   facteur.
2. **Version asymétrique** - le relais nécessite une révision ALPN que ce client n'offre pas
   (par exemple un relais réglé sur `control_alpn = "required"` et exécutant un protocole
   révision plus récente que ce client). Cela échoue bruyamment par conception, jamais de manière silencieuse
   downgrade : un client actuel contre un relais sur `lightspeed-control/1` négocie,
   et un relais exigeant tout ce que ce client ne propose pas le rejette avec 120.

**Comment affiner le résultat :**

- Exécutez `lightspeed-client --test-control --proxy <relay-ip>` pour voir à quelle étape cela
  le relais est activé. La valeur `alpn=` sur la ligne connectée et dans le résumé final est la
  Protocole d'application négocié : `lightspeed-control/1` signifie que le relais a accepté le
  offre versionnée, `none` signifie qu'il a accepté une offre vide après la solution de secours (une version plus ancienne)
  relais), et une exécution qui échoue avec le verdict de cette section est le cas ci-dessus.
- Si tous les relais tombent en panne sur ce réseau mais que les mêmes relais fonctionnent ailleurs, suspectez le
  middlebox : désactivez l'inspection HTTPS/QUIC du VPN ou du produit de sécurité et réessayez,
  puis attachez une capture UDP 4433 au #140.
- Si un seul relais tombe en panne, comparez le mode ALPN de ce relais avec la version de votre client : le
  La ligne de démarrage du relais et le champ `control_alpn` sur son `GET /health` signalent le
  mode configuré (voir [Référence CLI](CLI-REFERENCE.md#proxy-configuration)). Si vous
  faire fonctionner ce relais, `control_alpn = "off"` restaure le comportement précédent pour les anciens
  clients ; la procédure de l'opérateur et la règle de commande axée sur le client sont en
  [Deploy Proxy](deploy-proxy.md) et le [plan de déploiement ALPN](quic-alpn-rollout.md).

### Le jeu se déconnecte lorsque l'intercepteur démarre

L'intercepteur saisit les paquets avant que le jeu ne puisse recevoir des réponses et le chemin d'injection échoue.

**Fenêtres :**
1. Vérifiez que `WinDivert64.sys` et `WinDivert.dll` sont à côté du `.exe`.
2. Déconnectez les adaptateurs réseau secondaires (adaptateurs virtuels Docker, VMware, Hamachi)
3. Connectez-vous au serveur de jeu **avant** de démarrer l'intercepteur

**Linux :**
1. Vérifiez les règles nftables : `sudo nft list ruleset | grep lightspeed`
2. Si les règles sont obsolètes : `sudo lightspeed --check` pour diagnostiquer

### "Échec de l'ouverture de WinDivert" / `FWP_E_IN_USE` (0x8032000A) sous Windows

Seuls les handles qui portent un vrai filtre de capture — l'interception (`udp and outbound …`) et l'ombre (`udp and inbound …`) — possèdent un callout/filtre WFP. Le handle d'épinglage (pin) lié à la durée de vie du processus et le handle d'injection utilisent le filtre `"false"`, ne détournent aucun trafic et ne possèdent aucun objet WFP. Un callout/filtre WFP n'est donc pas « un par handle ouvert », et un pin maintenu n'est pas un verrou d'état WFP : dans la trace terrain du 23/11/2026, le pin est ouvert et l'ouverture d'interception suivante échoue toujours avec `FWP_E_IN_USE`.

`FWP_E_IN_USE` signifie que l'objet WFP est encore référencé (par exemple le filtre d'un processus précédent que le noyau n'a pas fini de supprimer après un arrêt forcé), ou qu'un callout est en cours d'enregistrement/désenregistrement concurrent. « Aucun processus n'utilise WinDivert visiblement » ne prouve pas que rien ne détient la référence, et aucun état WFP n'est capturé aujourd'hui : ne traitez pas le texte de l'erreur comme une cause racine.

LightSpeed ferme les handles d'interception, d'injection et d'ombre sur chaque chemin d'arrêt ordonné (Ctrl+C dans `--watch`/`--start-interceptor`, **Quitter** dans l'interface), débloque la boucle de réception avec `WinDivertShutdown` avant la fermeture et attend l'accusé des threads propriétaires. Ce déterminisme porte sur la **propriété des handles** de l'application ; le teardown WFP côté noyau est un chemin distinct dont les échecs ne sont pas visibles depuis l'espace utilisateur.

Si le problème persiste :

1. **Relancez Démarrer une fois.** Le prochain correctif réessaie `FWP_E_IN_USE` automatiquement tant que le pin est maintenu (borné, ~5 s) et peut recycler le pin une fois ; la 1.12.5 ne réessaie que lorsqu'**aucun** pin n'est maintenu, donc y réessayer revient à Arrêter puis Optimiser à nouveau.
2. **Quittez proprement et attendez un instant** : utilisez Ctrl+C dans la CLI ou **Quitter** dans l'interface, puis laissez une ou deux secondes aux handles pour se fermer avant de relancer.
3. **Arrêtez le service WinDivert** (administrateur ; c'est une réinitialisation explicite que LightSpeed ne fait pas automatiquement, et elle affecte toute autre application basée sur WinDivert sur la machine — vérifiez d'abord qu'aucun autre client WinDivert ne tourne) :
   ```powershell
   sc stop windivert
   ```
4. **L'arrêt complet** est le dernier recours, et uniquement lorsqu'un utilisateur WinDivert externe ne peut pas être exclu : un **Redémarrer** de Windows peut réutiliser la session noyau qui détient l'état obsolète, donc un **Arrêter → rallumer** complet est le seul nettoyage fiable au niveau machine. Ce n'est pas la première solution et LightSpeed ne vous demandera pas de redémarrer sans cette raison.

WinDivert 2.2 garde son pilote chargé après la fermeture du dernier handle : le fournisseur WFP et ses 20 sous-couches subsistent (1 / 20) et `WinDivert64.sys` reste verrouillé jusqu'à l'arrêt du service ou le redémarrage de la machine. Le pin n'y change rien et ce n'est pas une garantie que le pilote se décharge lors d'un Stop.

> **Conseil :** dans les versions 1.2.2 et antérieures, un autre bug (l'authentification du plan de données rejetait tous les paquets, issue #59) gelait la connexion et forçait à tuer le client de façon répétée, ce qui a déclenché la plupart des rapports `FWP_E_IN_USE`. Ce bug d'authentification est corrigé en v1.2.3.

---

## Problèmes d'interface graphique Windows

Celles-ci s'appliquent à l'application `lightspeed-gui` sous Windows.

### Quitter n'a rien fait et a laissé un processus zombie

Sur les versions antérieures à la v1.4.2, choisir **Quitter** dans le menu de la barre d'état système pouvait laisser le processus s'exécuter en arrière-plan (un zombie), de sorte que la fenêtre se fermait mais que le moteur continuait de fonctionner et qu'un lancement ultérieur se comportait étrangement. La version 1.4.2 corrige ce problème : Quitter termine désormais le processus proprement.

Si vous utilisez une version plus ancienne et que le processus est bloqué, terminez-le manuellement :

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Ensuite, effectuez la mise à niveau vers la version 1.4.2 ou ultérieure.

### Une deuxième instance s'ouvre au lieu de concentrer la première

Sur les versions antérieures à la v1.4.2, le lancement de l'interface graphique empilait deux fois une deuxième fenêtre et un deuxième moteur. L'interface graphique contient désormais une garde à instance unique : un deuxième lancement affiche un bref avis "LightSpeed ​​est déjà en cours d'exécution" et se ferme au lieu de démarrer un autre moteur. Pour forcer quand même une deuxième instance (à des fins de diagnostic), transmettez `--force` ou définissez `LIGHTSPEED_GUI_FORCE=1`.

### Aucun relais découvert

L'interface graphique découvre les relais communautaires via le registre signé. Si la liste des relais reste vide :

1. Confirmez que vous avez accès à Internet et qu'un pare-feu ou un VPN ne bloque pas le HTTPS sortant vers le registre.
2. Vérifiez le journal de l'interface graphique (voir ci-dessous) pour détecter une erreur de récupération de registre ou de vérification de signature.
3. Sur la version CLI, exécutez `lightspeed-client --probe-proxies` pour afficher directement le rapport de découverte et d'analyse. Si la CLI ne trouve rien non plus, le problème est côté réseau et non spécifique à l’interface graphique.
4. Redémarrez l'interface graphique après avoir corrigé la connectivité ; la découverte s'exécute au démarrage.

### Comment trouver et ouvrir le journal de l'interface graphique

L'interface graphique écrit son journal de trace dans :

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Collez ce chemin dans la barre d'adresse de l'Explorateur de fichiers pour ouvrir le dossier, puis ouvrez `gui-trace.log` dans n'importe quel éditeur de texte. Joignez-le à un rapport de bug.

### "Heartbeat 0 in" dans le journal

Une ligne comme `Heartbeat 0 in` signifie que le moteur n’a envoyé aucun battement de coeur keepalive dans la fenêtre actuelle. En pratique, cela apparaît lorsque le client n'a pas encore établi de connexion fonctionnelle au plan de contrôle, donc aucun battement de cœur n'a été émis. Causes courantes :

- Le client ne s'est pas encore inscrit auprès d'un relais (vérifier la ligne d'inscription dans la vue statut).
- Le relais sélectionné est inaccessible.
- L'intercepteur n'a pas démarré, donc aucune session n'est active.

Une fois l’enregistrement réussi et les battements de cœur commencent à affluer, le compteur grimpe. S'il reste à 0 alors qu'un relais apparaît sain, exécutez `lightspeed-client --test-control` pour déterminer si le plan de contrôle est accessible.

---

## Journaux pour les rapports de bogues

Exécutez avec la journalisation du débogage pour capturer des diagnostics détaillés :

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Joignez le fichier journal à votre [problème GitHub](https://github.com/ShibbityShwab/lightspeed/issues).

---

## Toujours bloqué ?

- [FAQ](faq.md) - questions courantes
- [Problèmes GitHub](https://github.com/ShibbityShwab/lightspeed/issues) - rechercher les rapports existants
- Ouvrez un nouveau problème avec votre système d'exploitation, votre jeu et la sortie du journal