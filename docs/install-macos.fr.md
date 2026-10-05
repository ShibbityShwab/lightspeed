# Installer LightSpeed sous macOS

macOS est une plateforme où la CLI est privilégiée. L'interface graphique se compile pour macOS mais n'est pas testée sur du matériel réel, donc le client en ligne de commande (`lightspeed-client`) est la voie prise en charge.

---

## Quoi télécharger

Récupérez la dernière version depuis la [page Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Choisissez l'archive qui correspond au processeur de votre Mac :

| Votre Mac | Cible | Fichier |
|----------|--------|------|
| Apple Silicon (M1 et suivants) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

Vous ne savez pas lequel vous avez ? Lancez :

```bash
uname -m
```

`arm64` signifie Apple Silicon ; `x86_64` signifie Intel.

---

## Installer

L'installateur shell est le chemin le plus simple. Il détecte votre architecture et installe le client :

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Ou installez à la main depuis l'archive :

```bash
# Remplacez le nom de fichier par celui que vous avez téléchargé.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Vérifiez que le binaire se lance :

```bash
lightspeed-client --version
```

---

## Privilèges root

L'intercepteur macOS utilise `pfctl` (le filtre de paquets intégré) pour rediriger le trafic UDP du jeu. `pfctl` exige root, donc lancez le client avec `sudo` quand vous démarrez l'intercepteur :

```bash
sudo lightspeed-client --start-interceptor --game rust
```

La découverte, le sondage et les diagnostics (`--probe-proxies`, `--test-control`, `--check`) n'ont pas besoin de root.

---

## Premier lancement

Le client découvre les relais communautaires automatiquement via le registre signé. Aucune adresse de proxy n'est nécessaire.

```bash
# Sonde les relais communautaires et affiche un rapport
lightspeed-client --probe-proxies

# Démarre l'intercepteur pour votre jeu
sudo lightspeed-client --start-interceptor --game rust
```

---

## Vérifier que ça marche

**Vérifiez la découverte des relais.** Lancez :

```bash
lightspeed-client --probe-proxies
```

Cela effectue une passe de découverte/sondage et affiche un rapport listant chaque relais découvert et sa latence. Vous devriez voir les huit relais communautaires (Los Angeles, New Jersey, Singapour, Francfort, Tokyo, Mumbai, Madrid, Sydney).

**Vérifiez l'enregistrement sur le plan de contrôle.** Lancez :

```bash
lightspeed-client --test-control
```

Cela se connecte au plan de contrôle QUIC, enregistre une session, envoie un ping et se déconnecte, en affichant le résultat de chaque étape. Un enregistrement réussi prouve que le plan de contrôle est joignable et que l'authentification fonctionne.

**Vérifiez l'environnement.** Lancez :

```bash
lightspeed-client --check
```

Cela indique la disponibilité de l'intercepteur, l'état root, la détection du jeu et la joignabilité du proxy.

**Vérifiez le flux de paquets.** Une fois l'intercepteur lancé et votre jeu connecté à un serveur, les compteurs de paquets du client doivent grimper. Si « Paquets envoyés » reste à 0, l'intercepteur n'a pas encore vu le trafic du jeu ; voir [Dépannage](troubleshooting.md).

---

## Note sur Gatekeeper

Si macOS bloque le binaire avec « ne peut pas être ouvert car le développeur ne peut pas être vérifié », supprimez l'attribut de quarantaine :

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Ne faites cela que si vous avez téléchargé le binaire depuis la page Releases officielle.

---

## Étapes suivantes

- [Jeux pris en charge](supported-games.md)
- [Dépannage](troubleshooting.md)
- [Référence CLI](CLI-REFERENCE.md)
