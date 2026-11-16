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
# Check if the firewall rule exists
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
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
verbatim du client ; `{err}` est l'erreur QUIC sous-jacente et `{addr}` le
adresse du répondeur) :

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**Ce que le client a déjà fait.** Les clients actuels proposent toujours le nom ALPN versionné
`lightspeed-control/1`. Si le relais répond avec l'alerte TLS 120, le client réessaye
**une fois** avec une offre ALPN vide, puis se souvient de la réponse de ce relais pour le reste
du processus, donc dans le fonctionnement normal du client, la poignée de main supplémentaire vouée à l'échec se produit à
au maximum une fois par relais et par lancement. Un repli réussi est normal contre un relais qui
n'a pas été mis à niveau (le relais par défaut est `[network] control_alpn = "off"`) : le
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
   downgrade : un client actuel contre un relais sur `lightspeed-control/1` négocie,
   et un relais exigeant tout ce que ce client ne propose pas le rejette avec 120.

**Comment affiner le résultat :**

- Exécutez `lightspeed-client --test-control --proxy <relay-ip>` pour voir quelle étape cela
  le relais est activé. La valeur `alpn=` sur la ligne connectée et dans le résumé final est la
  protocole d'application négocié : `lightspeed-control/1` signifie que le relais a accepté le
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
1. Vérifiez que `WinDivert64.sys` et `WinDivert.dll` sont à côté du `.exe`
2. Déconnectez les adaptateurs réseau secondaires (adaptateurs virtuels Docker, VMware, Hamachi)
3. Connectez-vous au serveur de jeu **avant** de démarrer l'intercepteur

**Linux :**
1. Vérifiez les règles nftables : `sudo nft list ruleset | grep lightspeed`
2. Si les règles sont obsolètes : `sudo lightspeed --check` pour diagnostiquer

### "Échec de l'ouverture de WinDivert" / `FWP_E_IN_USE` (0x8032000A) sous Windows

WinDivert enregistre une légende/filtre WFP pour chaque handle ouvert. Si un handle n'est jamais fermé, cet état de filtre persiste et le `WinDivertOpen` suivant échoue avec `FWP_E_IN_USE` (0x8032000A) même si aucun processus n'utilise visiblement WinDivert.

Les versions récentes de LightSpeed ​​ferment à la fois les poignées de capture et d'injection sur chaque chemin d'arrêt ordonné, y compris Ctrl+C dans `--watch` et `--start-interceptor` et **Quit** dans l'interface graphique, et la boucle de réception est débloquée avec `WinDivertShutdown` avant la fermeture, le démontage est donc déterministe. Un arrêt brutal (`taskkill /f`, un crash ou la fermeture de la fenêtre de la console) peut toujours laisser le pilote WinDivert 2.2.x dans un état obsolète ; il s'agit d'une limitation du pilote en amont (basil00/WinDivert#294, #406) que l'espace utilisateur ne peut pas effacer une fois le processus terminé.

Si vous frappez toujours :

1. **Quittez gracieusement et attendez un moment** : utilisez la CLI Ctrl+C ou **Quitter** de l'interface graphique, puis donnez aux poignées une seconde ou deux pour se fermer avant de relancer.
2. **Arrêtez le service WinDivert** (évite un redémarrage dans certains cas ; notez que le pilote est partagé avec toute autre application basée sur WinDivert sur la machine) :
   ```powershell
   sc stop windivert
   ```
3. **Arrêt complet, pas de redémarrage** : le « redémarrage » de Windows peut réutiliser la session du noyau qui contient l'état obsolète ; un **arrêt → mise sous tension** complet l'efface.

> **Astuce :** Sur la version 1.2.2 et les versions antérieures, un bug distinct (authentification du plan de données rejetant tous les paquets, problème n°59) a gelé la connexion et forcé les utilisateurs à tuer le client à plusieurs reprises, ce qui a déclenché la plupart des `FWP_E_IN_USE` rapports. Ce bug d'authentification est corrigé dans la v1.2.3.

---

## Problèmes d'interface graphique Windows

Ceux-ci s'appliquent à l'application `lightspeed-gui` sous Windows.

### Quitter n'a rien fait et a laissé un processus zombie

Sur les versions antérieures à la v1.4.2, choisir **Quitter** dans le menu de la barre d'état système pouvait laisser le processus s'exécuter en arrière-plan (un zombie), de sorte que la fenêtre se fermait mais que le moteur continuait de fonctionner et qu'un lancement ultérieur se comportait étrangement. La version 1.4.2 corrige ce problème : Quitter termine désormais le processus proprement.

Si vous utilisez une version plus ancienne et que le processus est bloqué, terminez-le manuellement :

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Ensuite, effectuez la mise à niveau vers la version 1.4.2 ou ultérieure.

### Une deuxième instance s'ouvre au lieu de concentrer la première

Sur les versions antérieures à la v1.4.2, le lancement de l'interface graphique empilait deux fois une deuxième fenêtre et un deuxième moteur. L'interface graphique contient désormais une garde à instance unique : un deuxième lancement affiche un bref avis "LightSpeed ​​est déjà en cours d'exécution" et se ferme au lieu de démarrer un autre moteur. Pour forcer quand même une deuxième instance (pour les diagnostics), transmettez `--force` ou définissez `LIGHTSPEED_GUI_FORCE=1`.

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

Une ligne comme `Heartbeat 0 in` signifie que le moteur n'a envoyé aucun battement de coeur keepalive dans la fenêtre actuelle. En pratique, cela apparaît lorsque le client n'a pas encore établi de connexion fonctionnelle au plan de contrôle, donc aucun battement de cœur n'a été émis. Causes courantes :

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