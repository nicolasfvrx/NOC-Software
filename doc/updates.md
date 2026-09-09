# Mise à jour automatique au démarrage

NOC Manager, NOC Agent et NOC Display vérifient la dernière release stable de
`nicolasfvrx/NOC-Software` avant de charger leur configuration ou de se connecter.
La vérification utilise HTTPS et l’API GitHub. Elle n’effectue aucune mise à jour
pendant l’exécution normale : cette version ne fait pas de hot patch.

## Déroulement

1. L’application compare le numéro de la suite intégré dans son binaire au tag
   de la dernière release stable. Une version égale ou inférieure est ignorée.
   Les brouillons et préversions ne sont pas installés.
2. Elle choisit exactement l’exécutable brut correspondant à son application et à
   sa cible de build (Server 2012/2016 partagent le même binaire Windows ; Ubuntu
   22.04/24.04 restent distincts côté Agent). Chaque release publie ces exécutables
   nus (`noc-manager.exe`, `noc-display.exe`, `noc-agent-ubuntu-22.04-x64`,
   `noc-agent-ubuntu-24.04-x64`) en plus des archives d’installation complètes : la
   mise à jour n’ouvre donc aucune archive.
3. Elle vérifie la taille et le SHA-256 (digest fourni par GitHub) de l’exécutable
   téléchargé, puis son identité : nom de produit Windows attendu, ou en-tête ELF
   x86-64 sous Linux. Un asset sans digest GitHub ou de taille invalide est ignoré.
4. Elle conserve une copie de l’ancien exécutable, installe le nouveau et relance
   l’application. Les configurations, données Manager, images, profils Firefox et
   identifiants RDP ne sont jamais touchés : seul l’exécutable est remplacé.

La vérification et la préparation sont limitées à **45 secondes** au total. Une
absence de réseau, un refus GitHub, un fichier invalide, un manque de place ou de
droits laisse démarrer la version actuelle. Le redémarrage issu d’une mise à jour
ne refait pas immédiatement la même vérification.

Sous Linux, Python 3 utilise un remplacement atomique de l’exécutable, puis Rust
effectue `exec` : le PID est conservé, donc systemd ne démarre pas un deuxième
agent. Sous Windows, un assistant PowerShell sans fenêtre attend la fermeture
du processus initial, remplace le fichier puis le relance avec les mêmes arguments
et le même dossier de travail. Les droits du fichier installé sont conservés.
Si un autre écran verrouille l’EXE Windows, l’assistant relance la version disponible.

Le verrou de mise à jour évite deux remplacements simultanés dans une même
installation. Il ne termine jamais les processus des autres utilisateurs.

## Prérequis et configuration

- Windows : Windows PowerShell 3 ou plus récent et .NET 4.5 (présents sur la cible
  Server 2012). La connexion GitHub doit accepter TLS 1.2 avec des certificats valides.
- Linux : Python 3.9 ou plus récent, fourni notamment par les bases Ubuntu 22.04
  et 24.04. Aucun module Python externe n’est nécessaire.
- Le compte qui lance l’application doit pouvoir écrire dans le dossier du
  binaire et y créer `.noc-updates/`. Sous Linux, il doit aussi pouvoir conserver
  le propriétaire/groupe du fichier. Une installation `/opt/kiosk-agent` détenue
  par root reste en lecture seule pour un agent ordinaire : installer sous le compte
  de service voulu, ou effectuer ses mises à jour avec le compte propriétaire.
  Le mécanisme n’exécute ni `sudo`, ni élévation UAC, ni changement global de droits.
- Pour un dépôt privé, définir `NOC_GITHUB_TOKEN` dans l’environnement du compte
  ou du service : utiliser un token limité à **Contents: read** sur ce dépôt.
  Aucun token n’est intégré dans les builds ou écrit dans les journaux.

Les installations partagées par plusieurs utilisateurs doivent avoir une politique
de droits adaptée. La mise à jour ne rend pas le dossier accessible en écriture à
tous les utilisateurs pour contourner un refus d’accès.

Pour démarrer exceptionnellement sans vérification :

```powershell
./noc-manager.exe --no-update
./noc-display.exe --no-update
```

```bash
./noc-agent --no-update
```

`NOC_DISABLE_UPDATES=1` désactive également la vérification. Les commandes
`--version`, `-V` et `--set-credentials` ne déclenchent jamais de mise à jour.

## Versions et publication

Le fichier `VERSION` définit la version de suite des builds locaux. Lors d’un
build GitHub déclenché par un tag, les scripts intègrent le numéro du tag à la place.
Cela reste indépendant des versions applicatives des `Cargo.toml` : un Agent
1.0.0 peut appartenir à la suite 0.1.0.

Les scripts de packaging publient dix fichiers par release : les six archives
d’installation complètes (config d’exemple, images, documentation) et quatre
exécutables bruts dédiés à la mise à jour automatique. Les builds faits
directement avec Cargo utilisent `VERSION` et, faute de cible explicite,
choisissent Server 2012 pour Windows ou Ubuntu 22.04 pour Linux lors des
futures mises à jour. Préférer les scripts de build pour une installation.
Les builds de préversion ne s’auto-mettent pas à jour : le mécanisme est réservé
aux versions de suite stables à trois nombres.

**L’installation initiale de cette fonctionnalité reste manuelle** : les anciens
binaires ne contiennent pas de vérificateur et ne peuvent pas se mettre à jour
eux-mêmes. Une fois un binaire équipé installé, les prochaines releases stables
de numéro supérieur sont proposées automatiquement au prochain démarrage.

## Journaux et retour arrière

Le dossier `.noc-updates/` à côté du binaire contient le verrou, un journal minimal
`<application>.log` et la sauvegarde `<application>.previous`. Aucun argument de
commande, jeton, URL privée ou contenu de configuration n’est journalisé.
Une erreur de lancement détectée immédiatement entraîne une tentative de restauration.
Il n’y a pas encore de validation de santé après connexion RDP ou chargement Firefox.

Pour revenir manuellement à la sauvegarde, arrêter les instances concernées,
restaurer l’ancien fichier à son emplacement initial puis démarrer avec
`--no-update` ou `NOC_DISABLE_UPDATES=1`, le temps de corriger la release.
Les empreintes détectent une corruption et l’origine est celle de GitHub via HTTPS ;
ce mécanisme n’ajoute pas de signature de code indépendante de GitHub.
