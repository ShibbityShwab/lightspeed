# Installer LightSpeed sous Linux

> [!WARNING]
> Traduction assistée par machine, non relue par un locuteur natif. La [version anglaise](install-linux.md) fait foi.

Linux est une plateforme où la CLI est privilégiée. L'interface graphique se compile pour Linux, mais le client en ligne de commande (`lightspeed-client`) est la voie prise en charge pour les configurations sans interface et les utilisateurs avancés.

---

## Quoi télécharger

Récupérez la dernière version depuis la [page Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Choisissez l'archive qui correspond à votre processeur :

| Votre machine | Cible | Fichier |
|--------------|--------|------|
| x86_64 (Intel/AMD) | `x86_64-unknown-linux-gnu` | `lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz` |
| ARM64 (Ampere, Graviton, Raspberry Pi 4/5) | `aarch64-unknown-linux-gnu` | `lightspeed-client-...-aarch64-unknown-linux-gnu.tar.xz` |

Vous ne savez pas lequel vous avez ? Lancez :

```bash
uname -m
```

`x86_64` signifie Intel/AMD 64 bits ; `aarch64` ou `arm64` signifie ARM64.

---

## Installer

L'installateur shell est le chemin le plus simple. Il détecte votre architecture et installe le client :

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Ou installez à la main depuis l'archive :

```bash
# Remplacez le nom de fichier par celui que vous avez téléchargé.
tar -xf lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Vérifiez que le binaire se lance :

```bash
lightspeed-client --version
```

---

## Privilèges root

L'intercepteur Linux utilise `nftables` (ou `iptables`) pour rediriger le trafic UDP du jeu, ce qui exige root. Lancez le client avec `sudo` quand vous démarrez l'intercepteur :

```bash
sudo lightspeed-client --start-interceptor --game rust
```

La découverte, le sondage et les diagnostics (`--probe-proxies`, `--test-control`, `--check`) n'ont pas besoin de root.

Assurez-vous que `nftables` est installé si votre distribution ne le fournit pas par défaut :

```bash
# Debian/Ubuntu
sudo apt install nftables

# Fedora/RHEL
sudo dnf install nftables

# Arch
sudo pacman -S nftables
```

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

## Étapes suivantes

- [Jeux pris en charge](supported-games.md)
- [Dépannage](troubleshooting.md)
- [Référence CLI](CLI-REFERENCE.md)
