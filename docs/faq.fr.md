# Questions fréquentes

---

## Bases

### LightSpeed est-il vraiment gratuit ?

Oui. LightSpeed est gratuit pour un usage personnel et non commercial sous la LightSpeed Software License. L'usage commercial exige une licence payante - voir [LICENSE](../LICENSE). Vous faites tourner votre propre proxy sur un petit VPS (voir le [guide de déploiement](../infra/README.md)). Il n'y a ni abonnement, ni frais d'usage, ni palier payant.

### LightSpeed va-t-il me faire bannir ?

Non. LightSpeed utilise la même classe de pilote réseau au niveau du système d'exploitation (WinDivert/nftables/pfctl) que les autres outils de capture de paquets. Il ne modifie pas les fichiers du jeu, sa mémoire ni ses processus. Tous les grands systèmes anti-triche (EAC, VAC, BattlEye, Riot Vanguard) l'autorisent. Les serveurs de jeu voient votre vraie adresse IP : c'est un tunnel transparent, pas un VPN ni un anonymiseur.

### Pourquoi l'intercepteur a-t-il besoin de root/Administrateur ?

L'interception de paquets au niveau du noyau exige des privilèges élevés, pour la même raison que les VPN et les pare-feu. Sous Linux, il utilise nftables/iptables. Sous macOS, il utilise pfctl. Sous Windows, il utilise WinDivert (un pilote noyau signé). Sans root, vous pouvez quand même utiliser le mode redirection (`--game-server`).

### Quelles plateformes sont prises en charge ?

| Plateforme | Intercepteur | Mode redirection | Interface graphique |
|----------|-------------|---------------|-----|
| Windows 10/11 | ✅ WinDivert | ✅ | ✅ egui |
| Linux | ✅ nftables/iptables | ✅ | ❌ CLI uniquement |
| macOS | ✅ pfctl | ✅ | ❌ CLI uniquement |
| Linux ARM64 | ✅ | ✅ | ❌ |

---

## Comment ça marche

### Comment LightSpeed réduit-il vraiment le ping ?

LightSpeed ne rend **pas** votre trafic plus rapide : les paquets ne peuvent pas dépasser la vitesse de la lumière. Ce qu'il fait, c'est *router de façon proactive* votre trafic sur le chemin le plus rapide disponible, en évitant la congestion et les détours inutilement longs.

Votre FAI envoie les paquets par le chemin le moins cher pour *lui*, souvent congestionné ou tortueux. LightSpeed fait passer vos paquets par un proxy installé dans un grand centre de données relié directement par backbone aux régions des serveurs de jeu. Si ce chemin est plus court ou moins congestionné que celui par défaut de votre FAI, votre ping baisse et se stabilise. Gain typique : 10 à 40 ms.

### Mon ping a AUGMENTÉ. Pourquoi ?

Deux raisons fréquentes :

1. **Mauvais emplacement du proxy** : si le proxy est plus loin du serveur de jeu que votre chemin direct, le saut supplémentaire ajoute de la latence. C'est la cause la plus courante. Règle de base : choisissez le proxy le plus proche du **serveur de jeu**, pas le plus proche de vous.
2. **Proxy mal connecté** : tous les centres de données ne se valent pas. Un proxy n'aide que si ce centre de données est proche d'un backbone internet majeur ou d'un point d'échange. Un VPS bon marché dans la « bonne » ville mais branché sur un amont congestionné ou résidentiel peut être plus lent que votre route directe.

LightSpeed ne peut optimiser que la route qu'on lui donne. Si vous pointez vers un proxy mal placé, votre ping montera : c'est le comportement attendu, pas un bug.

### Quel proxy dois-je choisir ?

Le proxy le plus proche de la **région du serveur de jeu**. Exemples :
- Vous jouez sur des serveurs US West → prenez un proxy US West
- Vous jouez sur des serveurs à Singapour depuis l'Australie → prenez un proxy à Singapour
- Vous jouez sur des serveurs UE depuis l'Amérique du Nord → prenez un proxy à Francfort/Londres

### Une note sur la réalité du routage (BGP)

Le routage réel d'internet est régi par **BGP** (Border Gateway Protocol), autrement dit les contrats et politiques que les FAI et les fournisseurs de transit utilisent pour se transmettre le trafic. Vos paquets ne voyagent pas en ligne droite : ils suivent le chemin que décident les tables BGP et les accords de peering, et les fournisseurs priorisent ou dépriorisent régulièrement certaines routes pour des raisons de coût ou de politique.

Ce que ça signifie pour vous :

- Un proxy n'aide que s'il se trouve sur un chemin BGP *meilleur* que celui par défaut de votre connexion domestique, typiquement un centre de données proche d'un backbone majeur ou d'un point de peering.
- « Plus proche sur la carte » ne veut pas toujours dire « plus rapide sur le câble ».
- Les outils d'optimisation de route (LightSpeed inclus) estiment et réacheminent, mais le chemin physique est dicté en dernier ressort par les réseaux intermédiaires, que ni vous ni LightSpeed ne contrôlez.

### À quelle vitesse la détection automatique se fait-elle ?

En général 1 à 3 secondes après votre connexion à un serveur de jeu. L'intercepteur attend 3 paquets vers la même destination en 1,5 seconde avant de se verrouiller.

### Comment LightSpeed décide-t-il où ajouter des relais ?

Le proxy déduit le **pays** d'une adresse IP à partir d'une base DB-IP Lite stockée localement. Cela se fait en mémoire, de façon transitoire, à la création de la session, pour l'adresse source du client comme pour l'adresse de destination du serveur de jeu. Il compte ensuite les sessions par paire `(source_country, destination_country)`, pour que le réseau voie quelles paires de régions sont mal desservies. Une cellule est masquée tant qu'elle n'a pas au moins 3 sessions avant d'être exportée, et les statistiques publiques ne portent que des comptes de paires de régions grossiers (par exemple `mena-eu`). Les compteurs ne contiennent aucune IP brute, et aucune IP brute n'est exportée par le pipeline de placement.

---

## FEC (réparation de paquets)

### Qu'est-ce que la FEC ?

Forward Error Correction, ou correction d'erreurs sans retransmission. Le relais envoie une petite quantité de données redondantes
(jusqu'à environ 25 % avec la taille de bloc par défaut) en plus de vos paquets. Si l'un d'eux est
perdu, il peut être reconstruit à partir de la parité sans retransmission, donc la
récupération n'a pas besoin d'un aller-retour vers le serveur de jeu.

### Dois-je l'activer ?

Non. La réparation de paquets est active par défaut et s'adapte aux pertes mesurées : une ligne propre
ne génère pratiquement aucun surcoût, et la parité n'augmente que lorsque des paquets
sont réellement perdus. (Les versions plus anciennes exposaient un interrupteur manuel, le « Reliability
Shield » ; depuis la 1.7, la politique adaptative est toujours aux commandes.)

---

## Faire tourner un proxy

### Comment obtenir un nœud proxy ?

Vous n'avez rien à faire. LightSpeed est livré avec le réseau de relais communautaire par défaut : huit relais financés par des sponsors (Los Angeles, New Jersey, Singapour, Francfort, Tokyo, Mumbai, Madrid, Sydney) que le client découvre automatiquement via un registre signé. L'URL du registre et la clé publique de l'opérateur sont compilées dans le client, donc il n'y a ni configuration à faire ni fichier de config à créer.

Si vous voulez utiliser un autre registre, remplacez-le avec `--registry <url>` ou un bloc `[registry]` dans `lightspeed.toml`. Voir le [guide du réseau de relais communautaire](community-network.md).

### Dois-je faire tourner mon propre proxy ?

Non. Le réseau communautaire est celui par défaut et fonctionne dès l'installation. L'auto-hébergement reste entièrement pris en charge si vous voulez votre propre relais dédié : déployez un proxy léger (environ 500 Ko de RAM) sur n'importe quel VPS Linux. Voir le [guide de déploiement](../infra/README.md).

### Combien coûte un proxy ?

Rien si vous utilisez le réseau communautaire. Si vous auto-hébergez, un petit VPS coûte quelques dollars par mois, et le binaire proxy utilise environ 500 Ko de RAM, donc même la plus petite instance suffit largement. LightSpeed lui-même n'a aucun frais.

### Puis-je partager mon proxy avec des amis ?

Oui. Le proxy gère plusieurs sessions simultanées avec limitation de débit et authentification par client. Configurez les jetons dans `proxy.toml`.

---

## Dépannage

### « Aucun trafic de jeu détecté »

- Assurez-vous que votre jeu est bien connecté à un serveur (pas seulement au menu principal)
- Vérifiez que vous avez sélectionné le bon jeu (option `--game`)
- Essayez `--scan-processes` pour lister les processus de jeu en cours
- Si votre serveur utilise un port non standard, utilisez le mode serveur manuel (`--game-server`)

### « Intercepteur indisponible »

- Linux : assurez-vous de tourner en root et que nftables/iptables est installé
- macOS : pfctl est intégré mais exige root
- Windows : vérifiez que `WinDivert64.sys` et `WinDivert.dll` sont à côté du `.exe`

### Paquets envoyés mais non livrés

Vos paquets atteignent le proxy mais les réponses n'arrivent pas jusqu'à votre jeu. C'est en général un problème de pare-feu. LightSpeed tente d'ajouter les règles de pare-feu automatiquement. Si ça échoue, ajoutez à la main une règle UDP entrante pour `lightspeed` ou `lightspeed-gui.exe`.

---

## Confidentialité

### LightSpeed lit-il mon trafic de jeu ?

LightSpeed voit les en-têtes des paquets UDP (IP source/destination, port, taille) pour les router. Le contenu du jeu (positions des joueurs, chat, etc.) est chiffré par le protocole du jeu lui-même et n'est ni déchiffré ni journalisé. Voir la [Politique de confidentialité](privacy.md) complète.

### Y a-t-il de la télémétrie ?

La télémétrie est **active par défaut** depuis la v1.6.5. Elle envoie des métriques agrégées anonymisées (percentiles de RTT, gigue, statistiques FEC et les chiffres de latence directe/relayée/économisée) vers l'endpoint `/telemetry` du relais auquel vous êtes connecté (un relais communautaire ou sponsor, ou le vôtre si vous auto-hébergez). Aucune adresse IP, aucun jeton, aucun identifiant, aucune donnée de compte de jeu n'est collecté. Une cellule est masquée tant qu'elle n'a pas au moins 3 rapports ; ce seuil compte les rapports, pas les personnes distinctes, donc il ne garantit pas que 3 personnes différentes ont contribué. Les rapports sont envoyés en POST via HTTP en clair sur le port 8080, et l'endpoint n'est pas authentifié. Désactivez-la à tout moment avec `--no-telemetry`, `telemetry = false` sous `[general]` dans `lightspeed.toml`, ou la case **« Share anonymous latency stats »** de l'interface graphique. Voir la [Politique de confidentialité](privacy.md) et le [Dictionnaire de données](data-dictionary.md).

### Le proxy stocke-t-il mon adresse IP ?

Non. Le proxy traite votre IP source et l'IP de destination du serveur de jeu de façon transitoire, en mémoire, pour router les paquets et en déduire un pays pour l'analyse de placement. L'adresse elle-même n'est pas stockée. Ce qui est conservé, c'est un compteur agrégé de sessions par paire `(source_country, destination_country)`, avec un seuil de 3 sessions par cellule pour masquer les petites cellules, et les statistiques publiques ne portent que des comptes de paires de régions grossiers. Les compteurs de placement ne contiennent aucune IP brute, et aucune IP brute n'est exportée par le pipeline de placement ; le contrat de télémétrie du client reste inchangé. Notez que les journaux d'accès du relais peuvent contenir des IP de clients ; voir [Que journalise le proxy ?](#what-does-the-proxy-log).

### Que journalise le proxy ?

Les journaux d'exploitation écrits sur stdout (configurables via `RUST_LOG`) peuvent inclure les IP des clients, les heures de début et de fin de session, et les octets relayés. Les IP des clients n'y apparaissent que pour la limitation de débit et la détection d'abus. Ces journaux restent sur le relais qui a servi la session (un relais communautaire ou sponsor, ou le vôtre si vous auto-hébergez), ne sont pas exportés et ne sont pas joints aux compteurs de placement. La conservation est contrôlée par la configuration de journalisation de ce relais. Voir la [Politique de confidentialité](privacy.md).

---

## Autre

### Puis-je utiliser LightSpeed avec un VPN ?

En général non : les deux tentent d'intercepter le trafic réseau et entreront en conflit. Désactivez votre VPN avant d'utiliser LightSpeed.

### LightSpeed fonctionne-t-il avec Cloudflare WARP ?

Oui. Utilisez `--warp` pour activer WARP sur le segment proxy de la connexion. WARP peut gratter 5 à 10 ms sur le routage local du FAI. Combinez-le avec un proxy pour un bénéfice maximal.

### Où signaler des bugs ?

[Ouvrez un ticket sur GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Indiquez votre système d'exploitation, votre jeu et la sortie des journaux (lancez avec `RUST_LOG=debug` pour des journaux détaillés).
