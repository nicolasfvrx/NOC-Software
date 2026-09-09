# NOC Agent

Guide commun : [build Ubuntu/Zorin OS et création des archives](../doc/linux.md).

Nom affiché : **NOC Agent**. Produit de **Norfair Operation Center**.
`noc-agent --version` affiche le nom et la version, sans charger la configuration.
Le titre de fenêtre est `NOC Agent` et son identifiant est `noc-agent`.

Le lanceur fourni contient le nom et la description du logiciel en français et
en anglais. Pour l’installer dans le menu des applications :

```bash
sudo install -Dm644 noc-agent.desktop /usr/local/share/applications/noc-agent.desktop
sudo install -Dm644 assets/logo.png /usr/local/share/pixmaps/noc-agent.png
```

Pour une installation existante utilisant systemd, arrêter et désactiver
`kiosk-agent.service` avant d’activer `noc-agent.service`. Supprimer également
l’ancien autostart `~/.config/autostart/kiosk-agent.desktop` s’il existe.
Choisir soit le service, soit l’autostart, pour éviter deux instances.
Les chemins historiques de configuration, de profil Firefox et d’état sont
conservés, notamment pour éviter de rejouer une ancienne commande distante.

Agent kiosque pour **Zorin OS / Ubuntu**, execute dans chaque session
**xRDP / XFCE**. Projet Rust **independant de NOC Manager** (qui, lui,
tourne sous Windows et expose l'API de configuration).

NOC Agent :

* reste lance **en permanence** ;
* affiche un **ecran de statut plein ecran** pendant tous les etats
  d'attente ou d'erreur ;
* recupere la configuration du kiosque sur NOC Manager ;
* verifie en HTTP que la destination est reellement utilisable ;
* lance **Firefox (Flatpak) en mode kiosk** via geckodriver / WebDriver ;
* garde Firefox **cache derriere** l'ecran de statut pendant le chargement ;
* detecte **reellement** la fin du chargement (`document.readyState`
  + selecteur CSS optionnel) — jamais un `sleep()` arbitraire ;
* se masque **seulement a ce moment-la** ;
* **reapparait immediatement** si Firefox ferme, crashe ou si une erreur
  survient.

L'utilisateur ne voit donc jamais le bureau XFCE, le demarrage de Firefox,
une page blanche, une 404, une 500 ni le chargement de Grafana.

---

## 1. Flux

```
xRDP → XFCE → NOC Agent (plein ecran, always-on-top)
     → API NOC Manager → config trouvee
     → test HTTP de l'URL
     → Firefox lance DERRIERE NOC Agent
     → page en chargement (readyState surveille)
     → page prete
     → NOC Agent se masque → Firefox devient visible

Firefox ferme / crash / URL modifiee / cron
     → NOC Agent reapparait immediatement → message → nouvelle tentative
```

## 2. Structure du projet

```
noc-agent/
├── Cargo.toml
├── config.example.toml
├── firefox-flatpak-wrapper.sh
├── noc-agent.service     unite systemd utilisateur (restart_agent)
├── README.md
├── assets/
│   ├── background.jpg      (place-holder — a remplacer par votre visuel)
│   └── logo.png            (PNG transparent)
└── src/
    ├── main.rs        fenetre eframe + demarrage du worker tokio
    ├── app.rs         orchestrateur / machine a etats
    ├── state.rs       etats + tous les messages affiches
    ├── config.rs      config.toml + detection de l'utilisateur Linux
    ├── manager.rs     client API NOC Manager + test HTTP de la cible
    ├── commands.rs    commandes distantes + state.json + polling
    ├── firefox.rs     geckodriver / wrapper Flatpak / profil / arret cible
    ├── webdriver.rs   session WebDriver + detection de page prete
    ├── scheduler.rs   restart_cron
    ├── ui.rs          fenetre de statut + pont thread-safe
    └── logging.rs     kiosk-agent.log
```

Binaire produit : **`noc-agent`**.

## 3. Build

```bash
# Dependances de compilation (une seule fois, sur la machine de build)
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
    libgl1-mesa-dev libxkbcommon-dev

# Rust (si absent)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cd noc-agent
cargo build --release
# → target/release/noc-agent
```

Le meme binaire sert a **tous** les kiosques : l'identification se fait
automatiquement avec le nom de l'utilisateur Linux courant.

## 4. Installation sur Zorin OS

```bash
# 1. Binaire + assets partages
sudo install -Dm755 target/release/noc-agent /opt/kiosk-agent/noc-agent
sudo install -Dm755 firefox-flatpak-wrapper.sh /opt/kiosk-agent/firefox-flatpak-wrapper.sh
sudo install -Dm644 assets/background.jpg /opt/kiosk-agent/assets/background.jpg
sudo install -Dm644 assets/logo.png       /opt/kiosk-agent/assets/logo.png

# 2. Firefox : deja installe via Flathub
flatpak list --app | grep org.mozilla.firefox
# sinon :
flatpak install -y flathub org.mozilla.firefox

# 3. geckodriver
sudo apt update
sudo apt install -y geckodriver          # Zorin/Ubuntu recents
# variante Ubuntu : sudo apt install -y firefox-geckodriver
# si le paquet n'existe pas, installation manuelle :
#   curl -LO https://github.com/mozilla/geckodriver/releases/latest/download/geckodriver-v0.35.0-linux64.tar.gz
#   sudo tar -xzf geckodriver-*-linux64.tar.gz -C /usr/local/bin
#   sudo chmod +x /usr/local/bin/geckodriver
geckodriver --version

# 4. Configuration (par utilisateur kiosque)
mkdir -p ~/.config/kiosk-agent
cp config.example.toml ~/.config/kiosk-agent/config.toml
$EDITOR ~/.config/kiosk-agent/config.toml     # renseigner manager.url
```

Dans `~/.config/kiosk-agent/config.toml`, pointer les chemins partages :

```toml
[ui]
background = "/opt/kiosk-agent/assets/background.jpg"
logo       = "/opt/kiosk-agent/assets/logo.png"

[firefox]
wrapper = "/opt/kiosk-agent/firefox-flatpak-wrapper.sh"
```

Ordre de recherche de `config.toml` :
`$KIOSK_AGENT_CONFIG` → `./config.toml` → `~/.config/kiosk-agent/config.toml`
→ `<dossier du binaire>/config.toml` → `/etc/kiosk-agent/config.toml`.

## 5. Autostart XFCE / xRDP

Un fichier `.desktop` par utilisateur kiosque (ou dans
`/etc/xdg/autostart/` pour tous) :

```bash
mkdir -p ~/.config/autostart
cp noc-agent.desktop ~/.config/autostart/noc-agent.desktop
```

A repeter (ou copier) pour `kiosk-noc-1`, `kiosk-noc-2`, `kiosk-noc-3`…
Aucune configuration specifique par kiosque n'est necessaire : l'agent
lit `id -un` et interroge l'API correspondante.

```
kiosk-noc-1  →  GET /api/kiosk/kiosk-noc-1
kiosk-noc-2  →  GET /api/kiosk/kiosk-noc-2
kiosk-noc-3  →  GET /api/kiosk/kiosk-noc-3
```

Astuce : desactiver l'economiseur d'ecran / la mise en veille XFCE
(`xfce4-power-manager-settings`, `xset s off -dpms`) pour un vrai kiosque.

## 6. Configuration attendue cote NOC Manager

`GET http://192.168.10.64:8080/api/kiosk/kiosk-noc-1`

```json
{
  "username": "kiosk-noc-1",
  "name": "Grafana NOC",
  "url": "https://grafana.example.com/d/abc/noc?kiosk",
  "enabled": true,
  "restart_cron": "0 4 * * *",
  "ready_selector": ""
}
```

| Champ            | Obligatoire | Role |
|------------------|-------------|------|
| `username`       | non         | informatif |
| `name`           | non         | libelle affiche dans les logs / l'ecran |
| `url`            | **oui**     | destination affichee |
| `enabled`        | non (`true`)| `false` → ecran « Affichage desactive », Firefox ferme |
| `restart_cron`   | non         | redemarrage planifie de **Firefox** uniquement |
| `ready_selector` | non         | selecteur CSS supplementaire pour « page prete » |

* HTTP **404** sur cet endpoint → etat `CONFIG_NOT_FOUND`
  (« Kiosque non configure »), nouvelle tentative periodique.
* Manager injoignable **pendant que Firefox tourne** → l'affichage est
  **conserve**, l'incident est simplement journalise.

## 7. Detection reelle du chargement (WebDriver)

1. NOC Agent choisit un port libre et demarre **geckodriver** :
   `geckodriver --host 127.0.0.1 --port <libre> --binary <wrapper>`.
2. Firefox etant installe en **Flatpak**, `/usr/bin/firefox` n'existe pas.
   On ne le referencera donc jamais : geckodriver recoit le wrapper
   `firefox-flatpak-wrapper.sh`, qui fait
   `exec /usr/bin/flatpak run org.mozilla.firefox "$@"` et transmet les
   arguments Marionette de geckodriver.
3. Capacites de session :
   * `moz:firefoxOptions.binary` = wrapper ;
   * `args` = `--kiosk`, `-profile <profil persistant>` ;
   * `pageLoadStrategy = "none"` → `goto()` rend la main tout de suite,
     c'est **NOC Agent** qui pilote l'attente.
4. Toutes les 500 ms, un script est evalue dans la page :

   ```js
   if (document.readyState !== 'complete') return { ready: false, ... };
   if (selector) return { ready: document.querySelector(selector) !== null, ... };
   return { ready: true, ... };
   ```

   * `ready_selector` vide → `readyState === "complete"` suffit ;
   * `ready_selector` renseigne (ex. `.dashboard-container`) → il faut en
     plus que l'element existe : on attend que Grafana soit reellement
     affiche.
5. Condition atteinte → log `RUNNING page ready` → NOC Agent relache
   l'always-on-top puis **se masque**. Firefox apparait.
6. `load_timeout_seconds` depasse → `PAGE_TIMEOUT`, Firefox est arrete,
   nouvelle tentative.

**Si geckodriver est absent** : aucun delai arbitraire, aucun affichage
« a l'aveugle ». L'agent affiche « Composant Firefox indisponible »,
journalise le composant manquant et reessaie periodiquement.

### Profil Firefox persistant

`profile_dir` (defaut
`~/.var/app/org.mozilla.firefox/kiosk-agent-profile`) est reutilise a
chaque lancement : cookies, session Grafana, preferences et certificats
sont conserves. Pas de navigation privee, pas de suppression du profil.
Le chemin doit rester accessible depuis le bac a sable Flatpak — d'ou
l'emplacement par defaut sous `~/.var/app/org.mozilla.firefox/`.

## 8. Etats et messages

| Etat | Message principal |
|------|-------------------|
| `STARTING` | Initialisation du kiosque… |
| `CONNECTING_TO_MANAGER` | Connexion au serveur de configuration… |
| `FETCHING_CONFIG` | Recuperation de la configuration… |
| `CONFIG_NOT_FOUND` | Kiosque non configure |
| `DISABLED` | Affichage desactive |
| `CHECKING_TARGET` | Verification du service… |
| `TARGET_UNAVAILABLE` | Service indisponible |
| `STARTING_FIREFOX` | Demarrage de Firefox… |
| `WAITING_FIREFOX` | Preparation du navigateur… |
| `LOADING_PAGE` | La page est en cours de chargement… |
| `LOADING_SLOW` | Le chargement prend plus de temps que prevu… |
| `RUNNING` | *(NOC Agent est masque)* |
| `RESTARTING` | Actualisation de l'affichage… |
| `FIREFOX_CRASHED` | Firefox s'est arrete |
| `PAGE_TIMEOUT` | La page ne repond pas correctement |
| `MANAGER_UNAVAILABLE` | Serveur de configuration indisponible |
| `FIREFOX_COMPONENT_MISSING` | Composant Firefox indisponible |
| `REMOTE_RESTART_BROWSER` | Actualisation de l'affichage… |
| `REMOTE_RESTART_AGENT` | Redemarrage de l'agent… |

Trois lignes sont affichees sous le logo : message principal, message
secondaire (detail de l'erreur, `{username}`, cause HTTP…) et une
troisieme ligne dynamique (compte a rebours « Nouvelle tentative dans X
secondes », progression `12 s / 60 s`, URL en cours…). Un bandeau bas
rappelle en permanence l'utilisateur, l'etat courant et l'heure : il n'y
a jamais d'ecran muet.

### Messages progressifs de chargement

Definis dans `src/state.rs` (`LOADING_STEPS`), faciles a modifier :

| Temps ecoule | Message |
|--------------|---------|
| 0–5 s   | La page est en cours de chargement… |
| 5–15 s  | Chargement du tableau de bord… |
| 15–30 s | Le chargement prend plus de temps que prevu… |
| 30–45 s | Toujours en attente de la page… |
| 45–60 s | Finalisation de l'affichage… |
| timeout | La page ne repond pas correctement |

## 9. Test HTTP avant Firefox

| Reponse | Interpretation |
|---------|----------------|
| 2xx / 3xx | service accessible → on lance Firefox |
| 401 / 403 | accessible (Firefox possede peut-etre un cookie que l'agent n'a pas) |
| 404 / 410 | « Page introuvable — HTTP 404 » → **Firefox n'est pas affiche** |
| 5xx | « Erreur du service — HTTP 503 » → **Firefox n'est pas affiche** |
| timeout / DNS / connexion refusee | « Impossible de joindre le serveur » |

Dans tous les cas bloquants, NOC Agent reste affiche et reessaie toutes
les `manager.retry_seconds` secondes.

## 10. Polling, cron et arret de Firefox

* **Polling** (`manager.poll_seconds`, defaut 30 s) meme quand Firefox
  tourne :
  * `enabled` passe a `false` → NOC Agent revient, Firefox est ferme,
    « Affichage desactive » ;
  * `url` (ou `ready_selector`) change → NOC Agent revient,
    « Mise a jour de l'affichage… », Firefox ferme, nouvelle URL testee,
    Firefox relance ;
  * `restart_cron` change → pris en compte **a chaud**, sans redemarrer
    l'agent ;
  * manager injoignable → l'affichage en cours est conserve.
* **`restart_cron`** ne redemarre que **Firefox**. Les expressions a 5
  champs (`0 4 * * *`) sont acceptees. Un seul declenchement par minute
  (memorisation du dernier tir).
* **Arret de Firefox** : d'abord fermeture propre de la session WebDriver
  (10 s), puis attente, puis `kill` **cible** du geckodriver lance par cet
  agent. En tout dernier recours seulement, `flatpak kill org.mozilla.firefox`
  — qui n'agit que sur les instances Flatpak de l'utilisateur Linux
  courant (`force_kill = false` pour le desactiver).
  **Jamais de `pkill firefox`** : plusieurs sessions kiosque coexistent.

## 11. Logs

Fichier `kiosk-agent.log` (chemin dans `[log] file`, relatif au dossier de
`config.toml` ; repli sur `/tmp` si le dossier n'est pas inscriptible).
Format : `date heure [username] ETAT message`, egalement repris sur stdout.

```
2026-09-09 01:00:00 [kiosk-noc-1] STARTING application started
2026-09-09 01:00:01 [kiosk-noc-1] FETCHING_CONFIG manager request
2026-09-09 01:00:01 [kiosk-noc-1] CHECKING_TARGET HTTP 200
2026-09-09 01:00:02 [kiosk-noc-1] STARTING_FIREFOX geckodriver=/usr/bin/geckodriver ...
2026-09-09 01:00:04 [kiosk-noc-1] LOADING_PAGE readyState=loading
2026-09-09 01:00:07 [kiosk-noc-1] RUNNING page ready
2026-09-09 04:00:00 [kiosk-noc-1] RESTARTING cron triggered
```

Aucun secret n'est journalise (ni jetons, ni identifiants).

## 12. Architecture technique

* **Thread UI** (`main`) : eframe/egui uniquement — affichage, mise a jour
  des textes, `hide`/`show` de la fenetre. Aucune I/O bloquante.
* **Thread worker** : runtime tokio — API HTTP, test HTTP, geckodriver,
  WebDriver, polling, cron.
* Communication par un `UiBridge` (`Arc<Mutex<…>>` + `request_repaint()`).
* Fenetre : plein ecran, sans decoration, always-on-top pendant les etats
  d'attente, adaptee automatiquement a la resolution xRDP (le rendu est
  recalcule a chaque frame). Fond en mode **cover** (ratio conserve, crop
  centre, jamais etire), logo PNG alpha centre a taille proportionnelle.
* UI **native** : ni Electron, ni Node, ni WebView, ni HTML.

## 13. Depannage

| Symptome | Piste |
|----------|-------|
| « Composant Firefox indisponible » | `geckodriver --version` ; `flatpak info org.mozilla.firefox` ; `chmod +x firefox-flatpak-wrapper.sh` |
| « Kiosque non configure » | l'API renvoie 404 pour `id -un` — verifier le nom d'utilisateur cote NOC Manager |
| « Service indisponible — HTTP 404 » | l'URL du dashboard est erronee |
| Certificat auto-signe refuse | `[target] insecure = true` |
| Firefox reste devant l'ecran de statut | verifier que le gestionnaire de fenetres XFCE respecte `always-on-top` (compositeur actif) |
| Session Grafana perdue a chaque lancement | `profile_dir` doit etre sous `~/.var/app/org.mozilla.firefox/` |
| « Firefox est deja en cours d'execution » | l'agent arrete les instances Flatpak residuelles et supprime `.parentlock` / `lock` du profil avant chaque lancement (traces `FIREFOX_CLEANUP`) ; necessite `force_kill = true` |
| Barre XFCE visible pendant un redemarrage | l'ecran de statut est remis devant 700 ms avant l'arret de Firefox et reaffirme plein ecran + always-on-top pendant 2 s ; verifier que le compositeur XFCE est actif |

## 14. Commandes distantes (NOC Manager)

Deux commandes peuvent etre poussees par NOC Manager, recuperees pendant
le polling normal (`manager.command_poll_seconds`, defaut **5 s**).

```
GET  {manager_url}/api/kiosk/{username}/command
POST {manager_url}/api/kiosk/{username}/command/{id}/ack
```

Reponse avec commande :

```json
{ "command": { "id": 42, "action": "restart_browser" } }
```

Sans commande : `{ "command": null }`.

### Anti-double-execution

Chaque commande porte un **id numerique unique**. Le dernier id traite est
persiste dans `~/.local/state/kiosk-agent/state.json`
(`$XDG_STATE_HOME` respecte si defini) :

```json
{ "last_command_id": 42 }
```

* fichier absent → `last_command_id = 0` ;
* `command.id <= last_command_id` → commande **ignoree** ;
* ecriture **atomique** (`state.json.tmp` puis `rename`) : pas de
  corruption en cas d'arret brutal ;
* l'id est marque **avant** l'ACK et **avant** toute action destructive,
  donc un ACK en echec ne provoque jamais de re-execution — l'echec est
  simplement journalise.

### `restart_browser`

Log `received` → id ecrit dans `state.json` → ACK → l'ecran de statut
repasse au premier plan avec « Actualisation de l'affichage… /
Redemarrage du navigateur » → arret propre de la session WebDriver, de
Firefox puis de geckodriver → attente **2 s** (async) → relecture de la
configuration → controle `enabled` / URL / accessibilite → relance de
Firefox par la **logique existante** → attente reelle de page prete
(`readyState` + `ready_selector`) → masquage de l'agent.

L'agent **ne quitte pas**, ne touche pas a la session xRDP et ne lance
jamais une seconde instance de lui-meme ni de Firefox.

### `restart_agent`

Log `received` → id ecrit dans `state.json` **avant de quitter** → ACK →
« Redemarrage de l'agent… / Reinitialisation de l'affichage » → arret
propre de WebDriver / Firefox / geckodriver → l'agent se relance
**lui-meme** via `exec` (`Command::new(current_exe)...exec()`), memes
PID/argv/environnement.

`exec` remplace l'image memoire du processus courant sans passer par le
superviseur : le comportement est identique que l'agent tourne sous le
service systemd (§15) ou sous l'autostart XFCE (§5), qui lui ne relance
jamais un processus termine. La verification de mise a jour au demarrage
est sautee une fois pour ce redemarrage (`NOC_SKIP_UPDATE`).

### Priorite et concurrence

* La file de commandes est consultee a chaque point d'attente : compte a
  rebours, boucle de chargement de page (500 ms) et boucle de
  surveillance (1 s). Un `restart_browser` arrivant **pendant** le
  chargement annule proprement ce chargement.
* `restart_agent` est prioritaire sur tout, y compris sur un
  `restart_browser` deja en file.
* Un indicateur `browser_restart_in_progress` empeche un second
  redemarrage tant que le premier n'est pas termine ; l'id de la commande
  ignoree est malgre tout marque et acquitte.
* L'orchestrateur reste strictement sequentiel et ne detient qu'un seul
  `Browser` a la fois : deux Firefox ne peuvent pas coexister, quelle que
  soit la combinaison cron / URL modifiee / crash / commande distante.

### Endpoint pas encore disponible

L'API de commandes n'existe pas encore cote Manager. Un **404**, une
connexion refusee ou une reponse illisible ne cassent rien : l'incident
est journalise **une seule fois** (et une fois de plus au retour a la
normale), et le fonctionnement — configuration, affichage, Firefox —
continue normalement. Aucune erreur plein ecran n'est affichee pour cette
raison.

### Action inconnue

Une action non reconnue n'est pas executee, mais elle est marquee dans
`state.json` et acquittee pour eviter une boucle permanente :

```
REMOTE_COMMAND unknown action=something_unknown
```

### Logs

```
REMOTE_COMMAND id=42 action=restart_browser received
REMOTE_COMMAND id=42 saved locally
REMOTE_COMMAND id=42 ack success
BROWSER_RESTART begin source=remote
BROWSER_RESTART firefox stopped
BROWSER_RESTART starting firefox
BROWSER_RESTART page ready

REMOTE_COMMAND id=43 action=restart_agent received
REMOTE_COMMAND id=43 saved locally
REMOTE_COMMAND id=43 ack success
AGENT_RESTART cleanup begin
AGENT_RESTART re-exec /opt/kiosk-agent/noc-agent
```

## 15. Service systemd utilisateur

`restart_agent` relance l'agent lui-meme (`exec`, meme PID) : le service
systemd n'est pas necessaire a son fonctionnement. Il reste utile pour
relancer l'agent apres un **crash** (`Restart=always`, `RestartSec=2`),
ce qu'un simple autostart ne fait jamais.

```bash
mkdir -p ~/.config/systemd/user
cp noc-agent.service ~/.config/systemd/user/

systemctl --user daemon-reload
systemctl --user enable --now noc-agent.service
```

Verification / journal :

```bash
systemctl --user status noc-agent.service
journalctl --user -u noc-agent.service -f
```

Si le service est utilise, **retirer** l'autostart `.desktop` du §5 pour
ne pas lancer deux instances.

L'agent tourne dans la session XFCE/xRDP : si la session est detruite,
NOC Agent s'arrete avec elle, ce qui est le comportement voulu. L'unite
systemd utilisateur sert uniquement a le maintenir/relancer **tant que la
session existe**.
