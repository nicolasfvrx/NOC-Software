# NOC Display — shell Windows avec RDP intégré

Guide commun : [build Windows et création des archives](../doc/windows.md).

Application Rust native pour Windows MultiPoint Server 2012 x64 (NT 6.2),
développée sur Windows 11. **MsTscAx / mstscax.dll est réellement hébergé en
ActiveX dans une fenêtre enfant.** Aucun lancement de mstsc.exe, Explorer,
navigateur, .NET ou autre client RDP.

## Compilation

Installer Visual Studio 2022 Build Tools, « Développement Desktop en C++ »,
MSVC x64, Windows SDK et Rust via rustup. Depuis la racine du projet :

```powershell
cargo build --release --target x86_64-pc-windows-msvc
cargo test --target x86_64-pc-windows-msvc -- --test-threads=1
```

Résultat : `target\x86_64-pc-windows-msvc\release\noc-display.exe`.
L'écran de marque affiche en haut à droite `Build JJ/MM/AA HH:mm`, sans encadré.
Cette date est intégrée à la compilation (heure locale du poste de build),
également inscrite au démarrage dans le journal. Elle disparaît avec l'écran de
marque lorsque la session RDP est affichée. Le script `build.rs` utilise Windows
PowerShell uniquement sur le poste de compilation. Un build Cargo sans changement
réutilise le binaire existant et conserve donc sa date.
`rust-toolchain.toml` fixe **Rust 1.77.2** ; conserver aussi `Cargo.lock`.
[Rust 1.78 relève le minimum des cibles pc-windows à Windows 10](https://blog.rust-lang.org/2024/02/26/Windows-7/).
Le CRT est statique et le sous-système PE est Windows 6.02. Aucun runtime Rust
ni compilateur à installer sur le serveur.

Sur le poste actuel, l'outillage local ignoré par Git peut aussi être utilisé :

```powershell
$env:RUSTUP_HOME = "$PWD\.tools\rustup"
$env:CARGO_HOME = "$PWD\.tools\cargo"
& .\.tools\cargo\bin\cargo.exe build --release --target x86_64-pc-windows-msvc
```

## Livraison et configuration

Copier les trois fichiers du dossier `DisplayClient` dans `C:\DisplayClient`.
Cette installation est partagée par tous les comptes Windows :

```text
noc-display.exe
background.jpg
logo.png
```

Deux emplacements possibles pour `config.toml`, cherchés dans cet ordre :

1. **Par compte** : `%USERPROFILE%\config.toml` (ex. `C:\Users\ecran1\config.toml`).
   Prioritaire s'il existe — permet de particulariser un écran donné.
2. **Global** : `C:\DisplayClient\config.toml`, à côté de l'EXE. Utilisé en
   repli si le compte n'a pas son propre fichier. Un seul fichier pour toute
   la machine.

Avec la configuration RDP centralisée (`manager.provides_rdp`, voir plus bas),
le fichier global suffit pour toute une machine : il ne reste plus qu'à créer
chaque compte Windows et de l’associer au kiosque correspondant dans Manager,
sans aucun fichier à écrire par compte. Sans centralisation, chaque écran a
besoin de son propre fichier (serveur/utilisateur RDP différents).

Copier le modèle `config.example.toml` sous ce nom, à l'emplacement voulu,
puis adapter ses paramètres.
Renseigner `rdp.password` pour une connexion automatique sans provisionnement.

```toml
[rdp]
enabled = true
server = "192.168.10.50"
port = 3389
username = "display1"
password = 'REMPLACER_PAR_LE_MOT_DE_PASSE'
domain = ""
retry_seconds = 10

[ui]
connecting_text = "Connexion en cours…"
manager_connecting_text = "Connexion au Manager en cours…"
manager_unreachable_text = "Manager injoignable"
disconnected_text = "Connexion interrompue"
error_text = "Connexion impossible"
reconnecting_text = "Nouvelle tentative dans {seconds} secondes"
development_exit_enabled = true
```

`server` est un nom DNS ou une IP, sans `rdp://`, chemin ou identifiants. Il doit
correspondre au certificat du serveur. L'intervalle va de 1 à 3600 secondes.
Fichier UTF-8 limité à 64 Kio ; clés inconnues rejetées sans recopier
leur contenu dans les logs. Les anciens fichiers `kiosk-rpd-client.cfg` et
`display-client.conf` ne sont plus lus. Le nom Windows vient de `GetUserNameW`.

Les images sont cherchées à côté de l'EXE. La configuration est cherchée dans
`%USERPROFILE%\config.toml` puis, en repli, à côté de l'EXE — indépendamment
du dossier de travail. La configuration est relue avant chaque tentative
(chacun des deux emplacements, dans le même ordre) ; en mode RDP désactivé,
relancer après modification.
Les images sont chargées au lancement. Une image absente/invalide n'empêche pas
RDP. Un TOML absent/invalide produit Error puis une nouvelle lecture, sans sortie.

## Mot de passe et option DPAPI

`rdp.password` est utilisé directement au lancement et à chaque reconnexion,
sans saisie ni fichier `credentials.dat` nécessaire. Il est **en clair dans
le TOML** : limiter l'accès au fichier au compte concerné et aux administrateurs.
Il n'est jamais inclus dans les logs ou la représentation Debug de la configuration.
Les apostrophes TOML conservent les antislashs littéralement ; si le mot de passe
contient une apostrophe, utiliser une chaîne entre guillemets doubles avec les
échappements TOML appropriés. Une valeur vide est refusée.

Si `password` est absent, le logiciel utilise DPAPI comme auparavant.
Un mot de passe présent dans le TOML a priorité sur DPAPI. Pour ce mode chiffré,
supprimer la ligne `password` et suivre les étapes ci-dessous.

Sous **le compte Windows dédié à l'écran, sur la machine cible**, renseigner
d'abord serveur/port/domaine/utilisateur dans `%USERPROFILE%\config.toml`,
puis exécuter le même EXE partagé :

```powershell
Start-Process -FilePath 'C:\DisplayClient\noc-display.exe' -ArgumentList '--set-credentials' -Wait
```

Ce mode explicite demande le mot de passe deux fois dans une console locale,
**sans écho**, puis quitte. Il ne connecte pas RDP. Ne pas passer le mot de passe
en argument de commande. Le fichier chiffré est :

```text
%USERPROFILE%\.display-client\credentials.dat
```

`CryptProtectData` utilise le contexte utilisateur, sans mode machine. L'entropie
lie le fichier au serveur, port, domaine et utilisateur RDP. Refaire le
provisionnement après changement de ces paramètres. Ne pas copier simplement ce
fichier d'un autre compte/poste. `CredentialProvider::load_credentials` permet
de remplacer ultérieurement DPAPI.

`CryptUnprotectData` utilise `CRYPTPROTECT_UI_FORBIDDEN`. Credentials absents ou
indéchiffrables : Error et retry, jamais de demande de saisie en mode kiosk.
Les buffers DPAPI déchiffrés sont effacés avant libération. Le mot de passe est
nécessairement déchiffré en mémoire pour l'injection dans MsTscAx, qui possède ses
propres buffers. En mode DPAPI, il n'est pas persisté en clair ; en mode TOML,
il reste en clair dans le fichier et dans la configuration en mémoire.

Un mot de passe **centralisé dans NOC Manager** (voir la section suivante) est
injecté exactement comme un mot de passe TOML : même priorité sur DPAPI, mêmes
garanties d'absence dans les logs. DPAPI reste le filet de secours quand
Manager ne fournit pas de mot de passe pour ce kiosque.

## RDP centralisée (NOC Manager)

Désactivé par défaut (`manager.provides_rdp = false`) : historiquement,
Display n'a aucune dépendance à Manager. Une fois activé, `[rdp]
server/port/password/ignore_certificate_errors` du `config.toml` local sont
**ignorés** et récupérés depuis Manager (`GET /api/kiosk/{username}/rdp`) à
chaque tentative de connexion — `[rdp] enabled` reste un interrupteur local :
il faut les deux (local `enabled = true` **et** Manager `rdp.enabled = true`
pour ce kiosque) pour qu'une connexion soit tentée.

**`{username}` est le nom du compte Windows courant**
(`identity::current_username()`, le même identifiant déjà utilisé pour le
heartbeat), pas un champ de `config.toml`. Dans Manager, associer ce compte
Windows au kiosque via `display_username` ; s’il est vide, le nom de la session
Agent est utilisé. Une fois cette association renseignée et
`provides_rdp = true`, **aucun réglage RDP n'est nécessaire dans
`config.toml`** au-delà de `[rdp] enabled = true` — créer le compte Windows
avec le bon nom suffit. Combiné au `config.toml` **global** (voir "Livraison
et configuration" plus haut), ça n'exige plus qu'un seul fichier pour toute
la machine : chaque nouveau compte Windows créé avec le bon nom se connecte
automatiquement, sans aucun fichier à écrire pour lui.

Comportement :

* Manager injoignable, kiosque inconnu, ou réponse illisible → échec de la
  tentative en cours, retraité par le compte à rebours habituel (aucun état
  supplémentaire, comme toute autre erreur de préparation RDP).
* Manager répond avec `enabled: false` pour ce kiosque → écran « Bienvenue,
  votre poste est prêt » (le même que `[rdp] enabled = false` en local), pas
  une erreur : désactiver le RDP d'un poste depuis Manager ne doit pas
  déclencher de boucle de retry visible à l'écran.
* Manager répond avec `enabled: true` → `server`/`port`/
  `ignore_certificate_errors`/`password` remplacent les valeurs locales pour
  cette tentative ; `username` effectif = le compte Linux retourné par Manager,
  avec repli sur le compte Windows si la réponse ne contient pas de nom.
  `[rdp] username` du TOML reste utilisé si `provides_rdp = false`.

Lorsque Manager est activé mais arrêté, Display affiche d'abord
`manager_connecting_text` avec l'indicateur de chargement pendant 60 secondes.
Les tentatives continuent en arrière-plan. Au-delà de ce délai, l'écran affiche
`manager_unreachable_text` et le compte à rebours de la prochaine tentative.
Une réponse valide de Manager retire immédiatement cet état.

Implémenté en WinHTTP natif (`src/health.rs::fetch_rdp_config`), plafonné à
16 Kio de réponse. Voir aussi le
[README de NOC Manager](../noc-manager/README.md#rdp-centralisée-noc-display)
pour l'UX du mot de passe côté administration et le compromis de sécurité
qu'implique cette centralisation.

## États et reconnexion

| État | Affichage |
| --- | --- |
| Starting | Background/logo, « Initialisation… » |
| Connecting | Marque, texte de connexion, roue animée ; contrôle caché |
| Connected | Seulement la session RDP, à (0,0), sur toute la zone client |
| Reconnecting | Marque, texte de coupure, compte à rebours |
| Error | Marque, texte d'échec, compte à rebours |

**Seul OnLoginComplete autorise Connected.** OnConnected ne montre pas le
contrôle. [OnLoginComplete indique la fin de l'ouverture de session](https://learn.microsoft.com/en-us/windows/win32/termserv/imstscaxevents-onlogincomplete),
pas la fin du démarrage de toutes les applications du bureau distant.

Les callbacks d'erreur/déconnexion cachent le contrôle avant de poster les
événements, journaliser ou détruire COM. Le fond de marque est prêt derrière lui.
Le délai de détection d'une panne dépend de MsTscAx/TCP ; le masquage est immédiat
dès réception de l'événement.

La reconnexion native est désactivée. Chaque tentative recrée un contrôle propre.
Timer Win32 de 250 ms pour les échéances, watchdog de logon de **45 secondes**,
timer de 50 ms uniquement pour la roue. Aucun sleep dans la boucle applicative.
Les événements tardifs d'anciennes instances sont ignorés grâce à leur génération.

La résolution est fixée avant Connect. Une modification de la zone client cache
et déconnecte RDP puis déclenche une tentative à la nouvelle taille. Aucune API
Dynamic Resolution Update récente. Fenêtre principale borderless/topmost sur le
moniteur principal de la session ; pas de fullscreen natif ni Connection Bar.

Dans tous les états, un bandeau discret en haut à droite affiche la version et
la date de build (`NOC Display v0.1.0 — Build 09/09/26 12:00`), pour vérifier
à l'œil quel binaire tourne sur un écran.

## Sécurité et dialogues

Sans option particulière, `AuthenticationLevel = 1` exige l'authentification
du serveur. Pour accepter un certificat non fiable, ajouter sous `[rdp]` dans
`%USERPROFILE%\config.toml` : `ignore_certificate_errors = true`.
Cette option utilise `AuthenticationLevel = 0` : l'identité du serveur n'est
plus vérifiée. Elle est activée dans le modèle fourni à la demande de
l'utilisateur, mais vaut `false` si absente. Son activation est journalisée.
Pour le serveur xRDP cible, CredSSP/NLA est désactivé ; la négociation TLS reste
activée. La valeur 2 qui affiche une confirmation n'est jamais utilisée.

Initialisation xRDP avant `Connect()` : `Server`, `UserName`, `Domain` vide,
QueryInterface `IMsTscNonScriptable`, `put_ClearTextPassword`, puis
`AdvancedSettings7` (interface `IMsRdpClientAdvancedSettings6`) et
`EnableCredSspSupport = VARIANT_FALSE`, avec relecture obligatoire.
Le mot de passe provient du fournisseur existant : TOML si présent, sinon DPAPI.
Le champ `domain` est ignoré pour la connexion xRDP ; laisser `domain = ""`.
Avec `autorun=Xorg` et `require_credentials=true` côté xRDP, ces réglages visent
l'auto-login initial. La réussite effective doit être vérifiée sur le serveur.
Les logs comprennent `OnLogonError` (source et code), `ExtendedDisconnectReason`
et `GetErrorDescription` à la déconnexion avant destruction du contrôle.
Les propriétés de sécurité sont obligatoires : si elles échouent, pas de Connect.
Les propriétés de confort optionnelles peuvent manquer.

`AllowPromptingForCredentials = false`, prompts client et sauvegarde de credentials
par MsTscAx désactivés. Les redirections de presse-papiers, lecteurs, imprimantes,
ports, cartes à puce et périphériques dynamiques sont désactivées.
Un hook CBT du thread UI refuse la création des dialogues natifs `#32770` pendant
la vie du contrôle. Les demandes d'interaction provoquent Error, sans validation
de confiance par l'application. Voir [AuthenticationLevel](https://learn.microsoft.com/en-us/windows/win32/termserv/imsrdpclientadvancedsettings4-authenticationlevel)
et [AllowPromptingForCredentials](https://learn.microsoft.com/en-us/windows/win32/termserv/imsrdpclientnonscriptable5-allowpromptingforcredentials).

En mode strict, la chaîne de certificats doit être approuvée côté infrastructure. L'absence de
dialogues dans tous les cas réels doit encore être testée sur la version exacte
de MsTscAx de MultiPoint, notamment certificats et mots de passe expirés.

## Shell et sortie

Configurer le compte dédié pour lancer le chemin absolu
`C:\DisplayClient\noc-display.exe` comme shell personnalisé. Le logiciel ne
modifie pas automatiquement le shell/registre et ne lance jamais Explorer.
Configurer le mot de passe ou provisionner DPAPI et tester avant de remplacer
le shell habituel. Les écrans
système avant le lancement du processus et le bureau sécurisé Windows ne sont
pas contrôlés par une application Win32.

Avec `ui.development_exit_enabled = true`, **Ctrl+Shift+F12** ferme proprement
l'application, y compris lorsque RDP a le focus. Échap est aussi disponible si
la fonctionnalité Cargo `dev-escape` est compilée (par défaut).
Les anciens aperçus F1–F11 et Ctrl+Alt+Q ont été retirés de cette version RDP.

En production, mettre `development_exit_enabled = false` : raccourcis locaux et
Alt+F4/WM_CLOSE ignorés. La fermeture de session Windows reste possible.
On peut aussi enlever Échap du binaire :

```powershell
cargo build --release --target x86_64-pc-windows-msvc --no-default-features
```

Cette option Cargo seule ne désactive pas le raccourci de secours : utiliser
également la configuration ci-dessus.

## Logs, architecture et DLL

`%USERPROFILE%\display-client.log` : date/heure locale, transitions, HRESULT
et raisons numériques. Chaque compte Windows possède son propre journal à la
racine de son profil, à côté de `config.toml`. Le fichier est créé automatiquement.
Le profil doit être inscriptible pour avoir un journal. Aucun repli vers le dossier
partagé de l'EXE ; l'ancien journal éventuel y est conservé mais n'est plus alimenté.
Chaque nouvelle ligne comporte `[windows_user="ecran1" pid=1234]` : compte Windows
récupéré via `GetUserNameW` et identifiant du processus, y compris pour les erreurs
de configuration et le démarrage. Ce compte est celui qui exécute le kiosk,
indépendamment du compte RDP configuré. Les anciennes lignes restent inchangées.
Aucun mot de passe, buffer DPAPI ou extrait de TOML invalide n'est enregistré.

| Modules | Responsabilité |
| --- | --- |
| main.rs | Provisionnement, durée de vie OLE |
| app.rs, state.rs | Orchestration, transitions, retry/watchdog |
| window.rs | Shell Win32 et callbacks de vue |
| renderer.rs, assets.rs, scaling.rs, text.rs | Rendu existant WIC/Direct2D/DirectWrite |
| config.rs, credentials.rs, identity.rs, logging.rs | TOML, DPAPI, utilisateur, logs |
| rdp/active_x.rs, host.rs | Contrôle et conteneur OLE |
| rdp/events.rs, dialog_guard.rs | Événements COM et refus des dialogues |
| rdp/settings.rs, interfaces.rs, dispatch.rs | Propriétés, ABI typée et ownership |

[Détails COM/ActiveX](docs/COM.md).

**Aucune API obligatoire Windows 10/11 n'est utilisée dans le code applicatif.**
Rust 1.77.2 et les API/interfaces choisies visent NT 6.2 ou antérieur. Cela ne
certifie pas encore l'exécution sur le vrai MultiPoint : recette cible nécessaire.

DLL système : kernel32, user32, gdi32, ole32, oleaut32, advapi32, crypt32, d2d1,
dwrite, ntdll, bcrypt, winhttp. WIC charge windowscodecs via COM. MsTscAx charge mstscax.dll et les
composants RDP/sécurité fournis par le système cible. **Ne pas copier mstscax.dll
de Windows 11 vers Server 2012.** Ni ATL redistribuable, CRT externe, .NET ou WinUI.

## Heartbeat / supervision Prometheus

Désactivé par défaut (voir `[manager]` dans `config.example.toml`) : Display n'a
historiquement aucune dépendance à NOC Manager. Une fois activé, un thread dédié
envoie périodiquement (`manager.heartbeat_seconds`, défaut **30 s**) un `POST`
vers `http://{manager.host}:{manager.port}/api/heartbeat/display/{username}`
(`username` = nom de la session Windows courante) avec la version, la date de build et l'état
de connexion courant (`Connecting`, `Connected`, `Reconnecting`, `Error`…).

Implémenté avec WinHTTP natif (aucune dépendance HTTP tierce, cohérent avec le
reste du binaire) : voir `src/health.rs`. Toute erreur réseau est journalisée
(`Heartbeat HRESULT=...`) puis ignorée jusqu'au prochain envoi — jamais bloquant
pour l'affichage RDP. Côté NOC Manager, ce heartbeat alimente `/metrics` et
l'interface web — voir le
[README de NOC Manager](../noc-manager/README.md#supervision-prometheus--grafana).

## Validation

Tests automatisés : géométrie, TOML/mot de passe et erreurs expurgées, transitions/watchdog/retry,
DPAPI aller-retour lié au serveur, rendu natif, création/activation/resize/
destruction MsTscAx, dialogue natif refusé, connexion à un port local fermé.
Ce dernier vérifie les vrais OnConnecting puis OnDisconnected sans visibilité.
Il dure environ 40 secondes sur ce poste ; limite de test de 90 secondes.

Recette restante sur serveur de test puis MultiPoint 2012 :

1. Credentials valides + certificat approuvé : marque → Connecting → RDP seul.
2. Mauvaise IP/DNS/serveur arrêté : Error, tentatives répétées sans sortie.
3. Coupure en session puis redémarrage serveur : marque immédiate à l'événement,
   compte à rebours et retour automatique de RDP.
4. Certificat non fiable, mauvais nom, mauvais mot de passe/expiration : Error
   sans boîte interactive et sans contournement de confiance.
5. Images absentes séparément : fond noir/image restante, RDP toujours utilisable.
6. TOML incorrect puis corrigé : erreur propre, récupération au retry.
7. Changement de résolution et compte shell sans Explorer : plein écran continu.
8. Raccourcis activés puis désactivés, avec le focus dans la session RDP.

**Pas encore validés :** session réussie, coupure/reprise réelle, certificats
réels et exécution MultiPoint 2012. Ils nécessitent un serveur de test et le
provisionnement local des identifiants.
