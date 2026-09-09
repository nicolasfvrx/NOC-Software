# NOC Manager

Guide commun : [build Windows et création des archives](../doc/windows.md).

Petit gestionnaire de kiosques : un exécutable Rust tourne sur un serveur Windows,
expose une interface web d'administration et une API HTTP. Chaque kiosque Linux
(session xRDP) exécute `kiosk.sh`, qui interroge l'API et maintient Firefox ouvert
en mode kiosque sur l'URL configurée.

Usage prévu : réseau local uniquement.

```
[ Windows ]  noc-manager.exe  --->  http://0.0.0.0:8080
                                          |
                    +---------------------+---------------------+
                    |                     |                     |
              kiosk-noc-1           kiosk-noc-2             kiosk-hall
              (kiosk.sh)             (kiosk.sh)             (kiosk.sh)
```

## Contenu

| Fichier | Rôle |
|---|---|
| `src/main.rs` | démarrage, état partagé, serveur HTTP |
| `src/config.rs` | lecture de `config.toml` (créé s'il manque) |
| `src/models.rs` | structure `Kiosk` + validation |
| `src/storage.rs` | chargement / écriture atomique de `kiosks.json` |
| `src/api.rs` | API HTTP + token bearer optionnel |
| `src/commands.rs` | commandes distantes (`commands.json`, queue / get / ack) |
| `src/health.rs` | inventaire en mémoire, historique SQLite et rendu Prometheus (`/metrics`) |
| `src/web.rs`, `src/dashboard.rs`, `src/assets/` | formulaires, supervision et ressources web embarquées |
| `rust-toolchain.toml` | pin Rust 1.77.2 (compatibilité Server 2012) |
| `.cargo/config.toml` | CRT MSVC statique |
| `kiosk.sh` | client Linux |
| `config.example.toml`, `kiosks.example.json`, `commands.example.json` | exemples |

Aucun framework JavaScript ni service de base de données à installer. SQLite est
embarqué dans l’exécutable pour l’historique ; les configurations restent en JSON.

## Build Windows (cible Windows Server 2012)

Le projet est volontairement figé sur **Rust 1.77.2**, dernière version dont la
bibliothèque standard reste compatible NT 6.2. Le pin est déclaré dans
`rust-toolchain.toml`, donc `cargo` seul utilise déjà 1.77.2.

```
rustup toolchain install 1.77.2
rustup target add x86_64-pc-windows-msvc --toolchain 1.77.2
cargo +1.77.2 build --release --target x86_64-pc-windows-msvc
```

Binaire produit :

```
target\x86_64-pc-windows-msvc\release\noc-manager.exe
```

### Pourquoi 1.77.2

Depuis Rust 1.78, `x86_64-pc-windows-msvc` exige Windows 10 / Server 2016. Concrètement,
le binaire compilé avec un Rust récent importe `ProcessPrng` depuis `bcryptprimitives.dll`
(ainsi que `WaitOnAddress` et `SetThreadDescription`) : cette DLL n'existe pas sur
Server 2012, et le chargeur refuse donc de démarrer le programme.

Compilé avec 1.77.2, le binaire n'importe plus que des APIs disponibles depuis Vista
(`BCryptGenRandom`, `SystemFunction036`, SRW locks, IOCP), et son en-tête PE indique
subsystem 6.0.

Vérification possible sur le binaire produit :

```
dumpbin /imports noc-manager.exe | findstr /i "bcryptprimitives ProcessPrng WaitOnAddress"
```

Aucune ligne ne doit ressortir.

### CRT statique

`.cargo/config.toml` active `+crt-static` pour la cible MSVC :

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

Le `.exe` ne dépend donc d'aucun `vcruntime140.dll` / `api-ms-win-crt-*.dll` :
aucun Visual C++ Redistributable à installer sur le serveur.

### Dépendances et Cargo.lock

Toutes les versions sont résolues pour rester compilables avec 1.77.2 ; `axum` est en
0.7 (0.8 exige un Rust plus récent). Le `Cargo.lock` est en **format v3**, lisible par
Cargo 1.77.2 — ne pas le régénérer avec un Cargo récent sans précaution, sinon il
repasse en v4 et/ou tire des crates en edition 2024.

Pour le régénérer proprement :

```
cargo +1.77.2 generate-lockfile
```

Si cette commande échoue (une dépendance transitive trop récente exige `edition2024`),
passer par le résolveur MSRV du Cargo moderne, qui respecte `rust-version = "1.77.2"` :

1. ajouter temporairement `resolver = "3"` sous `[package]` dans `Cargo.toml` ;
2. `rm Cargo.lock && cargo +stable generate-lockfile`
   (doit afficher *Locking N packages to latest Rust 1.77.2 compatible versions*) ;
3. retirer `resolver = "3"` (Cargo 1.77.2 ne le connaît pas) ;
4. vérifier avec `cargo +1.77.2 build --release --target x86_64-pc-windows-msvc`.

Le fichier ainsi produit reste en v3.

## Déploiement Windows

Copier sur le serveur, dans un même dossier :

```
noc-manager.exe
config.toml       (copie de config.example.toml)
kiosks.json       (optionnel, créé automatiquement si absent)
commands.json     (optionnel, créé automatiquement si absent)
```

Puis :

```
noc-manager.exe
```

Sortie attendue :

```
[kioskmanager] data file: kiosks.json
[kioskmanager] api token: disabled
[kioskmanager] listening on http://0.0.0.0:8080
```

`config.toml` et `kiosks.json` sont cherchés dans le **répertoire courant** : si le
programme est lancé comme tâche planifiée ou service, définir le dossier de départ
sur celui du `.exe`.

Ouvrir le port 8080 en entrée :

```
netsh advfirewall firewall add rule name="NOC Manager" dir=in action=allow protocol=TCP localport=8080
```

## Configuration

`config.toml` :

```toml
[server]
listen = "0.0.0.0"
port = 8080
api_token = ""

[data]
file = "kiosks.json"
commands_file = "commands.json"

[health]
stale_after_seconds = 90
```

`api_token` vide = aucune authentification. S'il est renseigné, les routes `/api/*`
(dont `/metrics`) exigent l'en-tête `Authorization: Bearer <token>` (l'interface
web reste ouverte) ; reporter alors la même valeur dans `KIOSK_API_TOKEN` en haut
de `kiosk.sh`.

`health.stale_after_seconds` : au-delà de ce délai sans heartbeat, `noc_up` passe
à `0` dans `/metrics`. À garder à quelques fois l'intervalle de heartbeat des
agents/displays eux-mêmes (par défaut 30 s côté client).

## Interface web

`http://<serveur>:8080/`

Interface sombre en français, consultable depuis un navigateur moderne avec
JavaScript activé. CSS et JavaScript sont embarqués, sans CDN.

* **Accueil** (`/`) : cartes des kiosques avec états Agent et Display séparés,
  commandes en attente et nouveaux écrans Display à configurer.
* **Kiosques** (`/kiosks`) : recherche, filtres, ajout manuel avant toute connexion,
  modification, suppression et commandes de redémarrage.
* **Serveurs RDP** (`/rdp-servers`) : catalogue des destinations Linux réutilisables,
  avec nom, IP/DNS, port et réglage de certificat ; création et modification.
* **Supervision** (`/supervision`) : tous les clients détectés, y compris sans
  configuration, fiches par application/session, versions, builds et historique
  filtrable par application, session exacte, état et dates. Les dates utilisent
  le fuseau du navigateur. Pagination par groupes de 100 événements.

La barre commune compte les kiosques configurés, opérationnels, à vérifier et
désactivés, puis les **nouveaux écrans** : uniquement les Displays non associés.
Les Agents non associés restent consultables dans la supervision et l’historique,
mais ne sont jamais proposés comme nouveaux écrans. L’Agent démarre dans la session
Linux après connexion RDP ; son absence initiale est présentée comme une attente de démarrage.
Un kiosque opérationnel est activé et possède deux heartbeats récents : Agent
`RUNNING`, Display `CONNECTED`. « Jamais vu » ne permet pas de conclure que le
client est hors ligne : les heartbeats Display sont désactivés par défaut.

Les statuts s’actualisent toutes les 10 secondes sans remplacer les champs de
formulaire. Un échec d’actualisation est signalé. La première page de l’historique
s’actualise aussi ; les pages anciennes restent stables pendant la consultation.

Un Display non associé porte son nom de session Windows et un badge « Nouveau ».
Cliquer sur son nom ou « Configurer » ouvre directement `/kiosk/new?display=...`.
Le formulaire préremplit le nom du kiosque et la session Display. Le compte Linux
est saisi manuellement. Il active le kiosque et le RDP centralisé, propose
le port 3389 et laisse la planification vide. Le serveur Linux et l’URL sont
signalés « À compléter ». Saisir le mot de passe RDP ou choisir explicitement les
identifiants déjà enregistrés sur le Display. Aucune information inconnue n’est inventée.
Un Display déjà associé ouvre l’édition de son kiosque, pour éviter les doublons.
La création manuelle reste disponible. Supprimer une configuration conserve les
heartbeats et fait réapparaître son Display comme nouvel écran.

### Catalogue de serveurs RDP

Dans un kiosque, sélectionner un serveur existant ou utiliser « Créer un serveur
ici ». La création enregistre et sélectionne la destination sans perdre les autres
champs du formulaire. Le compte Linux et le mot de passe restent propres au kiosque.
Le catalogue ne contient aucun compte ni mot de passe.

Le fichier `rdp_servers.json` est indépendant des kiosques ; son chemin se règle
avec `[data] rdp_servers_file`. Il est créé au premier enregistrement. Une référence
facultative `rdp.server_id` lie un kiosque au catalogue. Modifier une adresse, un port
ou le réglage de certificat est pris en compte à la prochaine connexion RDP des
Displays associés, sans interrompre les sessions en cours.

Les anciennes configurations gardent leur adresse via « Adresse spécifique à ce
kiosque ». Elles peuvent être rattachées au catalogue en édition. Une référence
inconnue est signalée : Manager ne renvoie pas une ancienne destination par défaut.
Inclure `rdp_servers.json` dans les sauvegardes du Manager.

Les formulaires regroupent le nom, les sessions Agent/Display, l’URL, l’activation,
la planification et le RDP. `display_username` est facultatif : vide ou absent,
il reprend `username`. Chaque session Display ne peut appartenir qu’à un kiosque.

Règles de validation :

* `username` : obligatoire, unique, caractères `A-Z a-z 0-9 - _ .`
* `name` : obligatoire
* `url` : obligatoire, doit commencer par `http://` ou `https://`
* `restart_cron` : optionnel ; si présent, 5 champs cron
* `rdp.server`/`rdp.port` : obligatoires si `rdp.enabled` ; `server` doit être
  un nom DNS ou une IP, sans `://`, `@` ni `/`

Chaque modification réécrit `kiosks.json` via un fichier temporaire renommé
(écriture atomique), pour éviter toute corruption.

## API

### `GET /api/health`

```json
{ "status": "ok" }
```

### `GET /api/kiosk/{username}`

```
curl http://192.168.10.64:8080/api/kiosk/kiosk-noc-1
```

```json
{
  "username": "kiosk-noc-1",
  "name": "Monitoring réseau",
  "url": "https://grafana.example.com/d/network",
  "enabled": true,
  "restart_cron": "0 4 * * *"
}
```

Kiosque désactivé : réponse identique avec `"enabled": false`.
Sans `restart_cron` : `"restart_cron": null`.

Kiosque inconnu — HTTP 404 :

```json
{ "error": "kiosk_not_found" }
```

### `GET /api/kiosk/{username}/rdp`

Endpoint **séparé** de `GET /api/kiosk/{username}` ci-dessus : celui-là est
récupéré (et potentiellement loggé) par NOC Agent sur les postes Linux, il
ne doit jamais transporter de mot de passe. `{username}` = le nom du compte
Windows du poste NOC Display qui interroge. La recherche utilise
`Kiosk.display_username`, ou `Kiosk.username` si ce champ est absent.

```
curl http://192.168.10.64:8080/api/kiosk/kiosk-noc-1/rdp
```

```json
{
  "enabled": true,
  "server": "192.168.10.50",
  "port": 3389,
  "username": "kiosk-noc-1",
  "password": "•••••••",
  "ignore_certificate_errors": true
}
```

`password` est `null` si aucun mot de passe n'est centralisé pour ce
kiosque — Display retombe alors sur son DPAPI local. Kiosque inconnu — même
`404 { "error": "kiosk_not_found" }` que ci-dessus.

## RDP centralisée (NOC Display)

Chaque Display interroge `/api/kiosk/{username}/rdp` avec **le nom de son propre
compte Windows**. Manager résout l’association `display_username` et retourne
`Kiosk.username` comme utilisateur de connexion Linux. Exemple : Display
`ecran-accueil`, Agent `linux-accueil`. Les heartbeats et la récupération de
configuration conservent leurs identités respectives. Mettre à jour Display
pour utiliser des noms différents en RDP centralisé : les anciennes versions
ignorent le nom Linux de la réponse. Les installations de même nom restent compatibles.

Champs du formulaire d'édition (section « RDP (NOC Display) ») :

* **Gérer la connexion RDP depuis Manager** (`rdp.enabled`) : interrupteur
  principal
* **Serveur RDP**, **Port** (défaut 3389)
* **Ignorer les erreurs de certificat**
* **Mot de passe** : centralisé, optionnel

UX du mot de passe, pensée pour ne jamais l'exposer par accident :

* jamais pré-rempli dans le formulaire, même en édition ;
* laisser le champ **vide** = mot de passe inchangé (une simple relecture du
  formulaire ne peut donc pas l'effacer par erreur) ;
* case à cocher explicite **« Supprimer le mot de passe centralisé »** pour
  l'effacer volontairement ;
* un badge indique s'il y en a un actuellement, sans jamais afficher sa
  valeur.

**Compromis de sécurité assumé.** Sans mot de passe centralisé, Display
utilise son identifiant DPAPI local (`--set-credentials`), chiffré et lié à
la machine/au compte Windows — non transférable par le réseau. Centraliser
le mot de passe dans Manager change ce modèle : le secret transite en clair
sur le réseau (Manager ne sert qu'en HTTP) et est stocké en clair dans
`kiosks.json`. Pour tout déploiement qui centralise des mots de passe RDP :

* définir un `api_token` non vide dans `config.toml` (protège aussi
  `/api/kiosk/{username}/rdp`, comme le reste de l'API) ;
* envisager un reverse proxy TLS devant Manager si le réseau n'est pas
  entièrement maîtrisé.

La centralisation est adoptable kiosque par kiosque : ne pas définir de mot
de passe pour un kiosque donné laisse ce poste continuer d'utiliser son
DPAPI local, même avec `rdp.enabled` géré depuis Manager pour le reste
(serveur/port/certificat).

## Commandes distantes

Depuis l'interface web, chaque kiosque peut recevoir une commande ponctuelle, que
l'agent Linux récupère à son prochain passage. Deux actions, et seulement deux :

* `restart_browser` — redémarrage de Firefox
* `restart_agent` — redémarrage complet de l'agent

Toute autre valeur est rejetée en HTTP 400 `invalid_action`. Aucune commande shell
n'est configurable depuis l'API : l'action est une simple étiquette que l'agent
interprète lui-même.

Une seule commande en attente par kiosque. Une nouvelle commande remplace la
précédente et reçoit un nouvel `id` ; le compteur `next_id` ne recule jamais.
« En attente » signifie ici « pas encore acquittée par l'agent » — il n'y a pas de
suivi d'exécution.

Le cron `restart_cron` reste indépendant : ces commandes sont immédiates.

### `GET /api/kiosk/{username}/command`

```json
{ "command": { "id": 42, "action": "restart_browser" } }
```

Aucune commande — HTTP 200 :

```json
{ "command": null }
```

Kiosque inconnu — HTTP 404 `{ "error": "kiosk_not_found" }`.

### `POST /api/kiosk/{username}/command/{id}/ack`

L'agent confirme l'exécution ; la commande est alors retirée.

| Cas | Réponse |
|---|---|
| id correspondant | 200 `{ "status": "acknowledged" }` |
| aucune commande en attente | 404 `{ "error": "command_not_found" }` |
| id différent de la commande en attente | 409 `{ "error": "command_id_mismatch" }` |

L'acquittement d'une vieille commande ne supprime jamais une commande plus récente.

### `POST /api/kiosk/{username}/command`

Création côté administration, utilisée par l'interface web.

```
curl -X POST -H "Content-Type: application/json"      -d '{"action":"restart_browser"}'      http://192.168.10.64:8080/api/kiosk/kiosk-noc-1/command
```

Réponse HTTP 201 :

```json
{ "command": { "id": 42, "action": "restart_browser" } }
```

Erreurs : 404 `kiosk_not_found`, 400 `invalid_action`.

Ces trois routes sont sous `/api/`, donc soumises au même `api_token` que le reste
de l'API quand il est renseigné.

### Interface web

La liste affiche une colonne **Commande** (`restart_browser (#42)` ou *aucune*) et
deux boutons par kiosque :

* **Redémarrer Firefox** → `POST /kiosk/{username}/restart-browser`
* **Redémarrer l'agent** → `POST /kiosk/{username}/restart-agent`, précédé d'un
  `confirm()` navigateur

Les deux redirigent vers la liste avec le message « Commande envoyée ».

### commands.json

```json
{
  "next_id": 44,
  "pending": {
    "kiosk-noc-1": {
      "id": 42,
      "action": "restart_browser",
      "created_at": 1788912000
    },
    "kiosk-noc-2": {
      "id": 43,
      "action": "restart_agent",
      "created_at": 1788912030
    }
  }
}
```

Le fichier est créé vide (`{"next_id": 1, "pending": {}}`) s'il n'existe pas, et
réécrit par fichier temporaire + rename comme `kiosks.json`. S'il est illisible au
démarrage, le serveur ne plante pas : il journalise l'erreur, conserve le fichier
sous `commands.json.invalid` et repart d'un état vide.

Les journaux de commande :

```
COMMAND queued username=kiosk-noc-1 id=42 action=restart_browser
COMMAND replaced username=kiosk-noc-1 old_id=41 new_id=42
COMMAND fetched username=kiosk-noc-1 id=42
COMMAND ack username=kiosk-noc-1 id=42
COMMAND ack mismatch username=kiosk-noc-1 requested=41 pending=42
```

## Supervision Prometheus / Grafana

NOC Agent et NOC Display envoient chacun un heartbeat périodique (statut, version,
build). Manager conserve le dernier état en mémoire et sur disque, et chaque
événement dans SQLite. Les métriques `/metrics` gardent leurs noms et étiquettes.

Configuration facultative (les anciennes configurations restent valides) :

```toml
[health]
stale_after_seconds = 90
history_file = "health.sqlite3"
retention_days = 30
```

La rétention est limitée à 1–365 jours. Purge au démarrage puis toutes les heures ;
les recherches excluent immédiatement les événements au-delà de la rétention.
Le dernier état connu de chaque client reste conservé, même après purge, afin de
retrouver les clients non configurés. Les timestamps anciens ne sont pas rafraîchis
au redémarrage. Une association ne réécrit pas l’identité historique des événements.

Prévoir l’espace disque pour chaque heartbeat : 200 kiosques avec Agent et Display
à 30 secondes représentent environ 34,6 millions d’événements sur 30 jours.
Les index et SQLite ajoutent un coût de stockage ; mesurer le volume réel du parc.
Les pages libérées sont réutilisées, le fichier ne rétrécit pas automatiquement.
Pour une sauvegarde simple, arrêter Manager puis copier `health.sqlite3` avec les
fichiers JSON ; ne pas copier uniquement le fichier SQLite pendant une écriture WAL.

En cas d’échec de stockage, les statuts continuent en mémoire et un avertissement
indique que l’historique est incomplet. Les événements perdus ne sont pas rejoués.
Si la base ne peut pas être ouverte au démarrage, corriger l’accès puis redémarrer.

Les routes web `GET /ui/status` et `GET /ui/history` alimentent l’interface selon
son accès existant (interface web ouverte, token réservé à `/api/*` et `/metrics`).
Elles n’exposent aucun mot de passe RDP. L’historique accepte `app`, `username`,
`state`, `from`/`to` (secondes Unix) et `before` (curseur retourné dans `next`).

Validation : `cargo +1.77.2 test --locked`, puis, depuis la racine,
`python scripts/test-manager.py noc-manager/target/x86_64-pc-windows-msvc/debug/noc-manager.exe`
après compilation avec cette cible. Le test HTTP utilise un dossier temporaire
et 200 kiosques fictifs ; il ne contacte aucune installation existante.

### `POST /api/heartbeat/{app}/{username}`

`app` vaut `agent` ou `display`. N'exige pas que le kiosque existe déjà dans
`kiosks.json` : la supervision fonctionne même pour une instance pas encore
enregistrée. Body JSON, tous les champs optionnels :

```
curl -X POST -H "Content-Type: application/json" \
     -d '{"version":"1.0.0","build":"09/09/26 12:00","state":"RUNNING"}' \
     http://192.168.10.64:8080/api/heartbeat/agent/kiosk-noc-1
```

### `GET /metrics`

```
# HELP noc_up 1 if a heartbeat arrived within the staleness window, 0 otherwise.
# TYPE noc_up gauge
noc_up{app="agent",username="kiosk-noc-1"} 1
# HELP noc_last_seen_seconds Unix timestamp of the last heartbeat received.
# TYPE noc_last_seen_seconds gauge
noc_last_seen_seconds{app="agent",username="kiosk-noc-1"} 1788912000
# HELP noc_info Build metadata of the last heartbeat received. Value is always 1.
# TYPE noc_info gauge
noc_info{app="agent",username="kiosk-noc-1",version="1.0.0",build="09/09/26 12:00",state="RUNNING"} 1
```

Exemple de scrape config Prometheus :

```yaml
scrape_configs:
  - job_name: noc-manager
    scrape_interval: 30s
    static_configs:
      - targets: ["192.168.10.64:8080"]
    # Seulement si api_token est renseigné dans config.toml :
    # authorization:
    #   credentials: <token>
```

Dans Grafana, une requête `noc_up == 0` (ou `time() - noc_last_seen_seconds > seuil`)
alerte sur un agent/display disparu ; `noc_info` donne les versions déployées par
kiosque, utile pour repérer un poste resté sur une ancienne version.

## Côté Linux (Zorin / Ubuntu)

### Installation des dépendances

```bash
sudo apt update
sudo apt install curl jq firefox -y
```

Si Firefox est fourni en snap et pose problème en kiosque, le paquet `firefox-esr`
ou le tarball Mozilla conviennent aussi ; seule la commande `firefox` doit exister.

### Installation du script

```bash
sudo cp kiosk.sh /usr/local/bin/kiosk.sh
sudo chmod +x /usr/local/bin/kiosk.sh
sudo sed -i 's|^KIOSK_SERVER=.*|KIOSK_SERVER="http://192.168.10.64:8080"|' /usr/local/bin/kiosk.sh
```

Le script détermine seul le compte via `id -un` : le même fichier sert à tous les kiosques.

### Exemple d'exécution

```bash
$ /usr/local/bin/kiosk.sh
2026-09-09 08:00:01 [kiosk-noc-1] starting, server http://192.168.10.64:8080
2026-09-09 08:00:01 [kiosk-noc-1] configuration loaded
2026-09-09 08:00:01 [kiosk-noc-1] restart schedule: 0 4 * * *
2026-09-09 08:00:01 [kiosk-noc-1] launching Firefox: https://grafana.example.com/d/network
2026-09-09 08:31:12 [kiosk-noc-1] URL changed
2026-09-09 08:31:12 [kiosk-noc-1] stopping Firefox (pid 4211)
2026-09-09 08:31:16 [kiosk-noc-1] launching Firefox: https://grafana.example.com/d/network2
2026-09-10 04:00:03 [kiosk-noc-1] cron restart triggered
```

Journalisation dans un fichier :

```bash
/usr/local/bin/kiosk.sh >> ~/kiosk.log 2>&1
```

### Démarrage automatique dans la session xRDP

Le script doit tourner **dans** la session graphique. Le plus simple est un
lanceur autostart, créé une fois par compte kiosque :

```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/kiosk.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=Kiosk
Exec=/usr/local/bin/kiosk.sh
X-GNOME-Autostart-enabled=true
EOF
```

Ne pas utiliser `crontab`, `/etc/crontab` ni de timer systemd : toute la
planification est gérée par `kiosk.sh` à partir de `restart_cron`.

## Comportement de kiosk.sh

* `USERNAME="$(id -un)"`, appel de `$KIOSK_SERVER/api/kiosk/$USERNAME`
* API injoignable ou kiosque inconnu → log + nouvelle tentative toutes les 10 s
* `enabled=false` → Firefox arrêté, aucun lancement, la config continue d'être relue
* `enabled=true` → `firefox --kiosk "$URL"`, PID conservé
* configuration rechargée toutes les 30 s ; Firefox n'est **pas** relancé si rien n'a changé
* changement d'URL → arrêt propre, pause de 3 s, relance sur la nouvelle URL
* Firefox fermé ou planté → pause, rechargement de la config, relance si `enabled=true`
* arrêt : `SIGTERM` d'abord, `SIGKILL` seulement en dernier recours après 10 s

Réglages en haut du script : `KIOSK_SERVER`, `KIOSK_API_TOKEN`, `POLL_INTERVAL`,
`RETRY_INTERVAL`, `RESTART_DELAY`, `STOP_TIMEOUT`, `TICK`.

## restart_cron

Syntaxe classique à 5 champs : `minute heure jour-du-mois mois jour-de-semaine`.
Sont gérés : `*`, valeurs, listes `1,15`, plages `1-5`, pas `*/6`.

| Expression | Effet |
|---|---|
| `0 4 * * *` | tous les jours à 04:00 |
| `30 3 * * 1` | tous les lundis à 03:30 |
| `0 */6 * * *` | toutes les 6 heures |
| vide | aucun redémarrage planifié |

Le redémarrage ne concerne **que Firefox** : ni Linux, ni xRDP, ni la session
utilisateur ne sont touchés.

Anti-doublon : le script mémorise la dernière minute déclenchée
(`LAST_CRON_TRIGGER`, ex. `2026-09-09 04:00`) et ne redéclenche pas tant que la
minute n'a pas changé.

Attention : l'heure utilisée est celle du **poste Linux**, pas celle du serveur Windows.

## Limites connues (V1)

* pas de comptes administrateurs sur l'interface web (LAN uniquement)
* le suivi de Firefox repose sur le PID du processus lancé ; si un Firefox est déjà
  ouvert dans la session, le script ne le gère pas
* pas de remontée d'état des kiosques vers le serveur
