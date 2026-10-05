# Guide d'utilisation de LightSpeed

> [!WARNING]
> Traduction assistée par machine, non relue par un locuteur natif. La [version anglaise](user-guide.md) fait foi.

> Instructions pas à pas pour réduire votre ping avec LightSpeed.

---

## Comment LightSpeed fonctionne

Votre FAI fait passer le trafic de jeu par des chemins optimisés pour le coût, pas pour la vitesse. LightSpeed intercepte les paquets UDP de votre jeu et les tunnelise via un **relais**, un serveur léger installé dans un centre de données relié aux régions des serveurs de jeu par des connexions backbone haut débit. Par défaut, vous utilisez le **réseau de relais communautaire** (huit relais financés par des sponsors, découverts automatiquement via un registre signé, aucune configuration à faire). Vous pouvez aussi héberger votre propre proxy. Si ce chemin est plus rapide que la route par défaut de votre FAI, votre ping baisse.

```
Votre PC ──→ FAI (chemin lent) ──→ Serveur de jeu        ❌ Ping élevé
Votre PC ──→ Proxy LightSpeed (backbone rapide) ──→ Serveur de jeu   ✅ Ping bas
```

---

## Prérequis

- L'outil en ligne de commande `lightspeed` ou `lightspeed-gui` (Windows). Aucun proxy à configurer : le client découvre les relais communautaires tout seul.
- Pour le mode intercepteur : privilèges root/Administrateur
- Facultatif : votre propre nœud proxy si vous préférez l'auto-hébergement (voir [Déployer un proxy](deploy-proxy.md))

---

## De quelle application ai-je besoin ?

| Vous êtes sur | Téléchargement | Pourquoi |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui` (MSI ou ZIP) | L'interface graphique est une application autonome : elle inclut déjà le moteur client + le pilote WinDivert. Vous n'avez **pas** besoin de la CLI. |
| **Linux** | `lightspeed-gui` (ou `lightspeed-client`) | L'interface graphique fonctionne sous Linux (la zone de notification est un stub) ; la CLI est destinée aux utilisateurs avancés. |
| **macOS** | `lightspeed-client` | Pas encore d'interface graphique testée. L'interface graphique se compile pour macOS mais n'est **pas testée** sur du matériel réel. |
| **Hébergement d'un proxy** | `lightspeed-proxy` | Uniquement si vous faites tourner un nœud relais sur un VPS. |

> **Vous n'avez jamais besoin que d'un seul paquet.** Si vous jouez sous Windows, prenez `lightspeed-gui` et ignorez le reste. `lightspeed-client` est destiné aux utilisateurs Linux avancés et aux joueurs macOS ; `lightspeed-proxy` est destiné à l'auto-hébergement.

---

## Démarrage rapide (CLI - toutes plateformes)

### 1. Vérifiez votre environnement

```bash
lightspeed --check
```

Cela vérifie que votre système dispose des outils de filtrage de paquets requis (nftables/iptables sous Linux, pfctl sous macOS, WinDivert sous Windows).

### 2. Sondez vos relais

```bash
lightspeed --probe-proxies
```

Affiche la latence vers chaque relais découvert. Le client sélectionne automatiquement le plus rapide au premier lancement ; vous pouvez le remplacer en choisissant celui le plus proche de votre **serveur de jeu**, pas de votre position.

### 3. Démarrez l'intercepteur

```bash
# Linux/macOS (nécessite root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (nécessite Administrateur)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. Lancez votre jeu

Connectez-vous à n'importe quel serveur normalement. LightSpeed détecte automatiquement le serveur de jeu à partir des paquets sortants et commence le tunnel en quelques secondes.

### 5. Surveillez

La CLI affiche des statistiques en direct :
```
⚡ OPTIMISATION - 123.45.67.89:28015
Paquets envoyés : 142 | Paquets reçus : 139 | Paquets livrés : 139
```

---

## Démarrage rapide (interface graphique - Windows)

### 1. Téléchargez

Récupérez la dernière version depuis [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Extrayez tous les fichiers et gardez `WinDivert64.sys` et `WinDivert.dll` à côté de `lightspeed-gui.exe`.

### 2. Exécutez en tant qu'administrateur

Clic droit sur `lightspeed-gui.exe` → **Exécuter en tant qu'administrateur**. L'intercepteur a besoin d'un accès au niveau du noyau (comme un logiciel VPN).

### 3. Choisissez un relais et un jeu

L'interface graphique découvre les relais communautaires et sélectionne automatiquement le plus rapide au premier lancement. Vous pouvez remplacer le relais dans la liste déroulante, puis choisir votre jeu.

### 4. Cliquez sur **⚡ OPTIMISER MA ROUTE**

Le statut passe à « 🎯 Recherche de votre serveur de jeu… »

### 5. Lancez votre jeu

Connectez-vous à n'importe quel serveur. LightSpeed le détecte automatiquement en quelques secondes.

---

### Interface graphique macOS (non testée)

L'interface graphique se compile pour macOS mais n'est **pas testée** sur du matériel réel. La version publiée
fournit un simple `tar.xz` (cargo-dist 0.32 ne gère ni `.app` ni `.dmg`), donc pour
produire un vrai bundle, exécutez sur un Mac :

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

Cela crée `LightSpeed.app` et `LightSpeed-1.6.5.dmg`. L'application est signée
de façon ad hoc, donc le premier lancement demande un clic droit → Ouvrir (ou
`xattr -dr com.apple.quarantine LightSpeed.app`).

---

## Choisir le bon relais

| Vous êtes en | Serveur de jeu en | Meilleure région de relais |
|-----------|---------------|-------------------|
| Australie | US West | US West (Los Angeles) |
| Europe | US East | US East (New Jersey) |
| Asie du Sud-Est | Singapour | Singapour |
| Asie du Sud | Inde | Mumbai |
| Asie de l'Est | Japon | Tokyo |
| Amérique du Sud | US East | US East (New Jersey) |
| Partout | Même région | Le plus proche du serveur de jeu |

> **Règle de base :** choisissez le relais le plus proche du **serveur de jeu**, pas le plus proche de vous. Votre trafic fait PC → relais → serveur de jeu, donc c'est le segment relais vers serveur de jeu qui compte le plus.

---

## Correction d'erreurs sans retransmission (FEC)

La FEC ajoute environ 25 % de surcoût en bande passante pour récupérer les paquets perdus sans retransmission.

**Activez-la quand :**
- Vous subissez des pertes de paquets (micro-saccades, rubber-banding)
- Vous êtes en Wi-Fi avec des interférences intermittentes

**Désactivez-la quand :**
- Votre connexion est déjà saturée
- Vous êtes sur une connexion limitée ou à quota
- Vous avez moins de 0,1 % de pertes de paquets (aucun bénéfice)

```bash
# CLI : active la FEC avec la taille de bloc par défaut (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Taille de bloc personnalisée (K=8 → 12,5 % de surcoût)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## Avancé : mode serveur manuel

Si la détection automatique ne marche pas (ports personnalisés, jeu inhabituel) :

```bash
# Mode redirection : le jeu se connecte à localhost, LightSpeed transfère vers le vrai serveur
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

Configurez ensuite votre jeu pour qu'il se connecte à `127.0.0.1:<port>` (le port local que LightSpeed affiche).

---

## Changer de serveur en cours de session

LightSpeed détecte automatiquement quand vous vous déconnectez d'un serveur pour vous connecter à un autre. Le statut affiche brièvement « 🎯 Recherche de votre serveur de jeu… » puis se verrouille sur la nouvelle destination. Aucune action manuelle n'est nécessaire.

---

## Zone de notification (interface graphique Windows)

- Cliquez sur **×** pour réduire dans la zone de notification (cela ne quitte pas l'application)
- Double-cliquez sur l'icône en forme d'éclair pour la restaurer
- Clic droit pour Connecter / Déconnecter / Quitter rapidement

---

## Voir aussi

- [Référence CLI](CLI-REFERENCE.md) - chaque option expliquée
- [FAQ](faq.md) - questions fréquentes
- [Dépannage](troubleshooting.md) - résoudre les problèmes
- [Déployer un proxy](deploy-proxy.md) - faites tourner votre propre proxy
- [Jeux pris en charge](supported-games.md) - compatibilité des jeux
