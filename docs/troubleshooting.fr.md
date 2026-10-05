# Dépannage

---

## Diagnostic rapide

Lancez d'abord la vérification d'environnement intégrée : elle attrape la plupart des problèmes :

```bash
lightspeed --check
```

Elle vérifie : la disponibilité de l'intercepteur, les outils de filtrage de paquets, la résolution du profil de jeu et la connectivité au proxy.

---

## Problèmes fréquents

### « Intercepteur indisponible »

**CLI :** votre système n'a pas les outils de filtrage de paquets requis ou vous manquez de privilèges.

| OS | Requis | Comment corriger |
|----|----------|------------|
| Linux | nftables ou iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (intégré) + root | `sudo lightspeed ...` |
| Windows | pilote WinDivert + Administrateur | Clic droit → Exécuter en tant qu'administrateur |

Vérifiez avec :
```bash
lightspeed --check
```

### « Aucun trafic de jeu détecté » / Paquets envoyés reste à 0

**CLI :** l'intercepteur ne trouve pas les paquets du jeu dans la plage de ports attendue.

1. Assurez-vous que votre jeu est **connecté à un serveur** (pas seulement au menu principal ou au lobby)
2. Vérifiez le jeu avec `--scan-processes` :
   ```bash
   lightspeed --scan-processes
   ```
3. Essayez un autre profil de jeu ou utilisez le mode serveur manuel

**Interface graphique (Windows) :** attendez 15 secondes. Si la bannière orange « ⚠ Aucun trafic de jeu détecté » apparaît :
1. Ouvrez PowerShell en mode élevé :
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Utilisez le port affiché dans **Avancé → définir le serveur manuellement**

### « 🎯 Recherche de votre serveur de jeu… » ne se résout jamais

Le détecteur n'a pas vu 3 paquets vers la même destination en 1,5 seconde.

1. Assurez-vous d'être connecté à un serveur de jeu (déplacez votre personnage pour générer du trafic)
2. Si les paquets ne sont toujours pas détectés après 15 secondes, votre serveur est sur un port non standard : utilisez le mode serveur manuel
3. Arrêtez et redémarrez l'intercepteur après vous être connecté au serveur

### Paquets envoyés qui grimpent, paquets livrés = 0

Les paquets atteignent le proxy mais les réponses n'arrivent pas jusqu'à votre jeu. En général un problème de pare-feu.

**Linux :**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS :**
```bash
sudo pfctl -d  # Désactive temporairement pf pour tester
```

**Windows :**
```powershell
# Vérifie si la règle de pare-feu existe
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# L'ajoute à la main si elle manque
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

### La vérification de santé du proxy échoue

```bash
# Teste la connectivité
curl http://YOUR_PROXY_IP:8080/health

# Réponse attendue :
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Si c'est injoignable :
- Vérifiez que le proxy tourne : `systemctl status lightspeed-proxy`
- Vérifiez que le pare-feu autorise l'UDP 4434 et le TCP 8080
- Vérifiez les journaux du proxy : `journalctl -u lightspeed-proxy --tail 50`

### Le jeu se déconnecte quand l'intercepteur démarre

L'intercepteur s'empare des paquets avant que le jeu puisse recevoir les réponses, et le chemin d'injection échoue.

**Windows :**
1. Vérifiez que `WinDivert64.sys` et `WinDivert.dll` sont à côté du `.exe`
2. Déconnectez les adaptateurs réseau secondaires (adaptateurs virtuels Docker, VMware, Hamachi)
3. Connectez-vous au serveur de jeu **avant** de démarrer l'intercepteur

**Linux :**
1. Vérifiez les règles nftables : `sudo nft list ruleset | grep lightspeed`
2. Si les règles sont obsolètes : `sudo lightspeed --check` pour diagnostiquer

### « WinDivert open failed » / `FWP_E_IN_USE` (0x8032000A) sous Windows

WinDivert enregistre un callout/filtre WFP pour chaque handle ouvert. Si un handle n'est jamais fermé, cet état de filtre persiste, et le prochain `WinDivertOpen` échoue avec `FWP_E_IN_USE` (0x8032000A) alors qu'aucun processus n'utilise visiblement WinDivert.

Les versions récentes de LightSpeed ferment les deux handles, capture et injection, sur chaque chemin d'arrêt propre, y compris Ctrl+C dans `--watch` et `--start-interceptor` et **Quitter** dans l'interface graphique, et la boucle de réception est débloquée avec `WinDivertShutdown` avant la fermeture pour que le démontage soit déterministe. Un arrêt forcé (`taskkill /f`, un crash, ou la fermeture de la fenêtre de console) peut quand même laisser le pilote WinDivert 2.2.x avec un état obsolète ; c'est une limitation du pilote en amont (basil00/WinDivert#294, #406) que l'espace utilisateur ne peut pas lever une fois le processus disparu.

Si vous tombez quand même dessus :

1. **Quittez proprement et attendez un instant** : utilisez Ctrl+C dans la CLI ou **Quitter** dans l'interface graphique, puis laissez une seconde ou deux aux handles pour se fermer avant de relancer.
2. **Arrêtez le service WinDivert** (évite un redémarrage dans certains cas ; notez que le pilote est partagé avec toute autre application basée sur WinDivert sur la machine) :
   ```powershell
   sc stop windivert
   ```
3. **Arrêt complet, pas redémarrage** : le « Redémarrer » de Windows peut réutiliser la session noyau qui détient l'état obsolète ; un vrai **Arrêter → rallumer** le nettoie.

> **Astuce :** sur la v1.2.2 et antérieures, un bug distinct (l'authentification du plan de données qui rejetait tous les paquets, ticket #59) figeait la connexion et forçait les utilisateurs à tuer le client à répétition, ce qui a déclenché la plupart des signalements de `FWP_E_IN_USE`. Ce bug d'authentification est corrigé en v1.2.3.

---

## Problèmes de l'interface graphique Windows

Ceux-ci concernent l'application `lightspeed-gui` sous Windows.

### Quitter n'a rien fait et a laissé un processus zombie

Sur les versions antérieures à la v1.4.2, choisir **Quitter** dans le menu de la zone de notification pouvait laisser le processus tourner en arrière-plan (un zombie), donc la fenêtre se fermait mais le moteur continuait à tourner et un lancement ultérieur se comportait bizarrement. La v1.4.2 corrige cela : Quitter termine désormais le processus proprement.

Si vous êtes sur une version plus ancienne et que le processus est bloqué, terminez-le à la main :

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Puis mettez à jour vers la v1.4.2 ou plus récente.

### Une deuxième instance s'ouvre au lieu de focaliser la première

Sur les versions antérieures à la v1.4.2, lancer l'interface graphique deux fois empilait une seconde fenêtre et un second moteur. L'interface graphique tient désormais un verrou d'instance unique : un second lancement affiche un bref message « LightSpeed is already running » et se termine au lieu de démarrer un autre moteur. Pour forcer quand même une seconde instance (pour le diagnostic), passez `--force` ou définissez `LIGHTSPEED_GUI_FORCE=1`.

### Aucun relais découvert

L'interface graphique découvre les relais communautaires via le registre signé. Si la liste des relais reste vide :

1. Vérifiez que vous avez accès à internet et qu'un pare-feu ou un VPN ne bloque pas le HTTPS sortant vers le registre.
2. Consultez le journal de l'interface graphique (voir ci-dessous) pour une erreur de récupération du registre ou de vérification de signature.
3. Sur la version CLI, lancez `lightspeed-client --probe-proxies` pour voir directement le rapport de découverte et de sondage. Si la CLI ne trouve rien non plus, le problème vient du réseau, pas de l'interface graphique.
4. Redémarrez l'interface graphique après avoir corrigé la connectivité ; la découverte se fait au démarrage.

### Comment trouver et ouvrir le journal de l'interface graphique

L'interface graphique écrit son journal de trace dans :

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Collez ce chemin dans la barre d'adresse de l'Explorateur de fichiers pour ouvrir le dossier, puis ouvrez `gui-trace.log` dans n'importe quel éditeur de texte. Joignez-le à un rapport de bug.

### « Heartbeat 0 in » dans le journal

Une ligne comme `Heartbeat 0 in` signifie que le moteur a envoyé zéro battement de cœur de maintien de connexion dans la fenêtre en cours. En pratique, elle apparaît quand le client n'a pas encore établi de connexion fonctionnelle au plan de contrôle, donc aucun battement de cœur n'est parti. Causes fréquentes :

- Le client ne s'est pas encore enregistré auprès d'un relais (vérifiez la ligne d'enregistrement dans la vue d'état).
- Le relais sélectionné est injoignable.
- L'intercepteur n'a pas démarré, donc aucune session n'est active.

Une fois l'enregistrement réussi et les battements de cœur en route, le compteur grimpe. S'il reste à 0 alors qu'un relais apparaît sain, lancez `lightspeed-client --test-control` pour déterminer si le plan de contrôle est joignable.

---

## Journaux pour les rapports de bug

Lancez avec la journalisation de débogage pour capturer des diagnostics détaillés :

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Interface graphique Windows
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Joignez le fichier journal à votre [ticket GitHub](https://github.com/ShibbityShwab/lightspeed/issues).

---

## Toujours bloqué ?

- [FAQ](faq.md) - questions fréquentes
- [Tickets GitHub](https://github.com/ShibbityShwab/lightspeed/issues) - cherchez parmi les signalements existants
- Ouvrez un nouveau ticket avec votre système d'exploitation, votre jeu et la sortie des journaux
