# Installer LightSpeed sous Windows

Windows est la plateforme où l'interface graphique est privilégiée. Le paquet `lightspeed-gui` est une application autonome qui contient déjà le moteur client et le pilote WinDivert, donc vous n'avez jamais besoin de télécharger un client séparé.

---

## Quoi télécharger

Récupérez la dernière version depuis la [page Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Pour Windows x86_64 (tous les PC Intel et AMD actuels), choisissez l'un des deux :

| Fichier | À utiliser quand |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | Recommandé. S'installe dans Program Files, ajoute un raccourci dans le menu Démarrer et enregistre un désinstalleur. |
| `lightspeed-gui-...-windows-msvc.zip` | Portable. Décompressez où vous voulez et lancez `lightspeed-gui.exe`. |

Les deux contiennent la même interface graphique plus `WinDivert.dll` et `WinDivert64.sys`.

> **Windows sur ARM64 n'est pas encore une cible publiée.** Les versions publiées ciblent `x86_64-pc-windows-msvc`. Sur un appareil ARM64, la version x86_64 tourne dans la couche d'émulation de Windows, mais elle n'est pas testée sur du matériel ARM64 réel.

### CLI Windows (non prise en charge)

Une version en ligne de commande pour Windows est aussi publiée en zip (`lightspeed-client-...-windows-msvc.zip`). Elle est fournie pour les scripts et l'usage sans interface, mais elle n'est **pas prise en charge** : l'interface graphique est la voie recommandée sous Windows. Si vous l'utilisez, lancez-la depuis un terminal élevé (voir ci-dessous).

---

## Installer (MSI)

1. Téléchargez le `.msi`.
2. Double-cliquez dessus et suivez l'assistant. Windows SmartScreen peut signaler un éditeur inconnu ; la version est attestée par Sigstore, donc vous pouvez vérifier l'attestation si vous le souhaitez.
3. Lancez **LightSpeed** depuis le menu Démarrer.

## Installer (zip portable)

1. Téléchargez le `.zip`.
2. Clic droit dessus, choisissez **Extraire tout**, et extrayez vers un dossier où vous avez le droit d'écrire (par exemple `C:\LightSpeed`). Ne l'exécutez pas depuis l'intérieur du zip.
3. Lancez `lightspeed-gui.exe`.

---

## Privilèges d'administrateur

LightSpeed utilise le pilote WinDivert pour intercepter le trafic UDP du jeu. WinDivert exige des droits **Administrateur**.

- L'interface graphique demande l'élévation quand elle doit démarrer l'intercepteur. Acceptez l'invite UAC.
- Si vous utilisez la version CLI, lancez-la depuis un terminal **Administrateur** (clic droit sur Windows Terminal ou PowerShell, puis **Exécuter en tant qu'administrateur**).

Sans élévation, l'intercepteur ne peut pas s'attacher et vous verrez une erreur d'intercepteur dans la vue d'état.

---

## Premier lancement

1. Lancez l'interface graphique et acceptez l'invite UAC.
2. L'interface graphique découvre les relais communautaires automatiquement via le registre signé. Aucune adresse de proxy n'est nécessaire.
3. Choisissez votre jeu dans la liste.
4. Démarrez l'intercepteur, puis lancez votre jeu et connectez-vous à un serveur.

L'interface graphique affiche l'état des relais, l'état d'enregistrement et les compteurs de paquets pour que vous voyiez le trafic passer.

---

## Vérifier que ça marche

**Vérifiez la découverte des relais et l'enregistrement.** Ouvrez la vue d'état de l'interface graphique. Vous devriez voir les relais communautaires listés (Los Angeles, New Jersey, Singapour, Francfort, Tokyo, Mumbai, Madrid, Sydney) avec un état sain, et une ligne d'enregistrement indiquant que la poignée de main QUIC/auth a réussi. Si vous utilisez la version CLI, lancez :

```powershell
lightspeed-client.exe --probe-proxies
```

Cela effectue une passe de découverte/sondage et affiche un rapport listant chaque relais découvert et sa latence. Vous devriez voir les huit relais communautaires.

**Vérifiez l'enregistrement sur le plan de contrôle.** Avec la version CLI :

```powershell
lightspeed-client.exe --test-control
```

Cela se connecte au plan de contrôle QUIC, enregistre une session, envoie un ping et se déconnecte, en affichant le résultat de chaque étape. Un enregistrement réussi prouve que le plan de contrôle est joignable et que l'authentification fonctionne.

**Vérifiez le flux de paquets.** Dans l'interface graphique, les compteurs de paquets doivent grimper dès que votre jeu est connecté à un serveur. Si « Paquets envoyés » reste à 0, l'intercepteur n'a pas encore vu le trafic du jeu ; voir [Dépannage](troubleshooting.md).

---

## Journaux

L'interface graphique écrit un journal de trace dans :

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Collez ce chemin dans la barre d'adresse de l'Explorateur de fichiers pour l'ouvrir. Joignez-le à un rapport de bug si quelque chose ne va pas. Voir [Dépannage](troubleshooting.md) pour la signification des lignes du journal.

---

## Désinstaller

- **MSI :** Paramètres → Applications → Applications installées → LightSpeed → Désinstaller.
- **Zip :** supprimez le dossier extrait. Le fichier journal sous `%LOCALAPPDATA%\Lightspeed\` reste en place ; supprimez-le à la main si vous voulez une suppression complète.

---

## Étapes suivantes

- [Jeux pris en charge](supported-games.md)
- [Dépannage](troubleshooting.md)
- [Référence CLI](CLI-REFERENCE.md)
