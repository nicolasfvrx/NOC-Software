# NOC — Norfair Operation Center

NOC est une suite de trois applications pour administrer des écrans de tableaux
de bord : un serveur de configuration, un agent qui pilote Firefox dans une
session Linux, et un client Windows qui affiche cette session par RDP.

[Télécharger une version](https://github.com/nicolasfvrx/NOC-Software/releases) ·
[Compiler les applications](doc/README.md) ·
[Pousser le code et publier une release](doc/push-release.md)

## Les trois applications

| Application | Où elle tourne | Rôle |
| --- | --- | --- |
| **NOC Manager** | Serveur Windows | Interface web et API pour configurer les kiosques et envoyer les commandes de redémarrage. |
| **NOC Agent** | Chaque session Linux XFCE/xRDP | Récupère sa configuration, pilote Firefox et affiche les messages d’attente ou d’erreur. |
| **NOC Display** | Session Windows dédiée à un écran | Affiche la session Linux via un client RDP intégré, avec écran de connexion et reconnexion automatique. |

```mermaid
flowchart LR
    Admin[Administrateur] -->|Interface web| Manager[NOC Manager]
    subgraph Linux[Serveur Linux : une session par kiosque]
        Agent[NOC Agent] -->|Pilote et surveille| Firefox[Firefox en mode kiosque]
    end
    Agent -->|API HTTP : configuration et commandes| Manager
    Firefox -->|Charge la page| Dashboard[Tableau de bord web]
    Display[NOC Display sur Windows] -->|RDP via xRDP| Linux
    Display --> Ecran[Écran du NOC]
```

## Fonctionnement

1. Dans **NOC Manager**, l’administrateur crée un kiosque associé à un nom
   d’utilisateur Linux, puis choisit son URL, son activation et éventuellement
   une planification de redémarrage du navigateur.
2. **NOC Display** ouvre une connexion RDP vers le serveur Linux avec le compte
   configuré pour cet écran. Les paramètres RDP sont propres à chaque compte Windows.
3. Dans cette session Linux, **NOC Agent** identifie l’utilisateur courant et
   consulte `/api/kiosk/{username}` sur Manager. Il vérifie l’accessibilité de la
   destination avant de lancer Firefox Flatpak via geckodriver/WebDriver.
4. Pendant la connexion, le chargement ou une erreur, les applications présentent
   un écran de statut NOC. L’agent masque son écran de statut lorsque la page est
   prête, pour laisser apparaître Firefox.
5. L’agent relit régulièrement la configuration et les commandes. Il réagit aux
   changements d’URL, à la désactivation, aux redémarrages planifiés et à l’arrêt
   de Firefox. Display gère séparément les coupures et reconnexions RDP.

Manager définit **ce qu’affiche le navigateur**. Display utilise sa propre
configuration locale pour **la connexion RDP** ; il ne reçoit pas ses paramètres
RDP de Manager. Plusieurs comptes kiosques peuvent partager les mêmes machines
et les mêmes exécutables, avec une configuration d’affichage par utilisateur.

### Commandes distantes

- `restart_browser` : l’agent remet son écran de statut au premier plan, ferme
  Firefox et recharge la page.
- `restart_agent` : l’agent ferme le navigateur puis quitte ; le service
  utilisateur systemd le relance avec `Restart=always`.

La planification `restart_cron` redémarre uniquement Firefox. L’agent conserve
l’identifiant de la dernière commande traitée pour éviter de la rejouer.
Si Manager devient indisponible alors que Firefox fonctionne, l’agent conserve
l’affichage en cours et réessaie de joindre le serveur.

## Installer et configurer

Télécharger l’archive correspondant à la machine dans les
[releases GitHub](https://github.com/nicolasfvrx/NOC-Software/releases).

| Application | Configuration | Guide détaillé |
| --- | --- | --- |
| Manager | `config.toml` dans son dossier de travail ; données dans `kiosks.json` et `commands.json`. | [NOC Manager](noc-manager/README.md) |
| Agent | `~/.config/kiosk-agent/config.toml` ; URL de Manager et chemins des images/wrapper Firefox. | [NOC Agent](noc-agent/README.md) |
| Display | `%USERPROFILE%/config.toml` pour chaque compte Windows ; serveur, compte et identifiants RDP. Images à côté de l’EXE. | [NOC Display](noc-display/README.md) |

Commencer par Manager, configurer les comptes kiosques, installer l’agent dans
chaque session Linux, puis configurer Display pour ouvrir ces sessions.
L’agent utilise **Firefox Flatpak**, **geckodriver** et une session graphique.
Choisir son service systemd utilisateur ou l’autostart ; ne pas activer les deux.

Les chemins historiques `kiosk-agent` et `.display-client` restent utilisés pour
préserver les configurations, les profils Firefox et les secrets déjà installés.

## Plateformes et builds

Les nouvelles versions des trois applications incluent une
[mise à jour automatique au démarrage](doc/updates.md) depuis les releases stables.
La configuration reste conservée et un échec de vérification laisse démarrer la
version actuelle. Les premiers binaires équipés doivent être installés manuellement.

| Application | Exécutable | Cibles de la CI |
| --- | --- | --- |
| NOC Manager | `noc-manager.exe` | Windows Server 2012 et 2016 x64 |
| NOC Display | `noc-display.exe` | Windows Server 2012 et 2016 x64 avec bureau graphique |
| NOC Agent | `noc-agent` | Ubuntu 22.04 / Zorin OS 17 et Ubuntu 24.04 / Zorin OS 18 x64 |

Les builds Windows utilisent Rust **1.77.2** et le runtime MSVC statique. Les
builds Linux sont réalisés sur les deux bases Ubuntu. Les essais graphiques et
RDP sur les systèmes cibles restent nécessaires ; la CI compile et réalise des
contrôles de paquet, elle ne simule pas une installation complète du NOC.

- [Build Windows et création des archives](doc/windows.md)
- [Build Linux et installation](doc/linux.md)
- [GitHub Actions et publication automatique](doc/github-actions.md)
- [Commandes Git : premier push, mises à jour, tags](doc/push-release.md)

Les builds ne se déclenchent que sur un push de tag tel que **`v0.1.0`**, ou
manuellement via `workflow_dispatch`. Un tag stable déclenche aussi la création
de la release après réussite des six builds, avec quatre archives d'installation
Windows, deux archives Linux et quatre exécutables bruts pour la mise à jour
automatique. Un suffixe comme `v0.2.0-rc.1` crée une préversion.

Le tag représente la version de la **suite NOC**. Les versions internes restent
indépendantes : Manager **0.1.0**, Display **0.1.0**, Agent **1.0.0** pour cette
première livraison de la suite. Elles figurent dans les `Cargo.toml` et dans les
métadonnées ou la commande `--version` des applications concernées.

## Périmètre actuel

- Usage prévu sur un réseau local maîtrisé. L’interface web Manager n’a pas de
  comptes administrateurs et le serveur ne reçoit pas d’état de santé des agents.
- Manager propose un token API optionnel, mais le client Rust NOC Agent actuel
  n’envoie pas d’en-tête Bearer : cette association nécessite un token Manager vide.
- L’agent sait attendre un sélecteur CSS `ready_selector`, mais Manager ne permet
  pas encore de définir ni de renvoyer ce champ ; la détection standard de fin
  de chargement est utilisée avec le Manager actuel.
- Le script `noc-manager/kiosk.sh` est un client Linux simplifié fourni avec
  Manager. Le composant de la suite avec écran de statut est **NOC Agent**.

## Organisation du dépôt

```text
.github/workflows/   Builds et release GitHub
doc/                Guides de compilation et publication
scripts/            Scripts de build, packaging et publication
shared/             Mise à jour au démarrage partagée entre les applications
noc-manager/        Serveur d’administration Windows
noc-agent/          Agent graphique Linux
noc-display/        Client d’affichage RDP Windows
```
