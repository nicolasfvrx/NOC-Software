# Build Linux — NOC Agent

## Distributions couvertes

| Build x64 | Destination prévue |
| --- | --- |
| Ubuntu 22.04 | Ubuntu 22.04, Zorin OS 17 |
| Ubuntu 24.04 | Ubuntu 24.04, Zorin OS 18 |

Zorin utilise les bases Ubuntu correspondantes ; les builds CI sont réalisés sur
Ubuntu, puis restent à valider dans une session Zorin réelle. Pour une autre
version d’Ubuntu/Zorin, compiler sur cette version et vérifier les dépendances.
Les anciennes éditions et ARM ne font pas partie de la matrice automatique.

Le binaire Linux dépend des bibliothèques système (notamment glibc et OpenSSL).
Un build fait sur une distribution récente ne garantit pas l’exécution sur une
plus ancienne. Utiliser le paquet correspondant à la base Ubuntu du système.
Vérifier `/etc/os-release` (`VERSION_ID` et, sur Zorin, `UBUNTU_CODENAME`).

## Prérequis de compilation

Sur Ubuntu ou Zorin OS :

```bash
sudo apt-get update
sudo apt-get install -y curl build-essential pkg-config libssl-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxinerama-dev \
  libgl1-mesa-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  desktop-file-utils

# Si Rust n’est pas déjà installé :
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/noc-rustup.sh
sh /tmp/noc-rustup.sh -y --profile minimal
source "$HOME/.cargo/env"
rustup toolchain install stable --profile minimal
```

L’agent utilise Rust stable et son `Cargo.lock` version 4. La version effective de
Rust est enregistrée dans chaque archive. Contrairement aux builds Windows, le
compilateur Linux n’est pas figé : les sources des dépendances restent verrouillées,
mais deux builds éloignés dans le temps ne sont pas garantis identiques octet par octet.

## Compiler et créer le paquet

Depuis la racine `noc` :

```bash
bash scripts/build-linux.sh
```

Le script détecte le nom de la distribution, compile pour
`x86_64-unknown-linux-gnu`, exécute `noc-agent --version`, valide le lanceur avec
`desktop-file-validate`, vérifie les dépendances avec `ldd`, puis crée une archive
dans `dist/`. Aucune session graphique n’est nécessaire pour ces vérifications.

L’archive contient le binaire, les images, le wrapper Firefox, les fichiers
`.desktop` et `.service`, la configuration exemple, la documentation, les
informations de build, la liste des bibliothèques liées et l’empreinte SHA-256.

Pour compiler uniquement le binaire :

```bash
cd noc-agent
rustup target add --toolchain stable x86_64-unknown-linux-gnu
cargo +stable build --release --locked --target x86_64-unknown-linux-gnu
./target/x86_64-unknown-linux-gnu/release/noc-agent --version
```

## Installation depuis une archive

Extraire l’archive, puis se placer dans son dossier :

```bash
sudo install -Dm755 noc-agent /opt/kiosk-agent/noc-agent
sudo install -Dm755 firefox-flatpak-wrapper.sh /opt/kiosk-agent/firefox-flatpak-wrapper.sh
sudo install -Dm644 assets/background.jpg /opt/kiosk-agent/assets/background.jpg
sudo install -Dm644 assets/logo.png /opt/kiosk-agent/assets/logo.png
sudo install -Dm644 noc-agent.desktop /usr/local/share/applications/noc-agent.desktop
sudo install -Dm644 assets/logo.png /usr/local/share/pixmaps/noc-agent.png
mkdir -p ~/.config/kiosk-agent
# Première installation uniquement ; conserver la configuration si elle existe.
test -f ~/.config/kiosk-agent/config.toml || cp config.example.toml ~/.config/kiosk-agent/config.toml
```

Adapter l’URL du Manager et les chemins des images/wrapper dans la configuration.
L’exécution nécessite une session graphique XFCE/xRDP, Firefox Flatpak
`org.mozilla.firefox`, geckodriver accessible dans le PATH et les bibliothèques
listées par `ldd`/`DEPENDENCIES.txt`. La compilation CI ne fournit pas Firefox ni
geckodriver et ne lance aucune session RDP.

Installer geckodriver selon les [instructions Mozilla](https://firefox-source-docs.mozilla.org/testing/geckodriver/Support.html).
Les noms de paquets APT peuvent varier selon la distribution.

Le [README de NOC Agent](../noc-agent/README.md) décrit la configuration, le service
utilisateur et l’autostart. Dans une archive, ce README est fourni à côté de ce guide.
Choisir le service ou l’autostart et désactiver l’ancien `kiosk-agent.service`
avant d’activer `noc-agent.service`, pour éviter les doubles instances.

Avant déploiement, valider dans une vraie session Ubuntu/Zorin : apparition de
l’écran NOC Agent, chargement de Firefox, page prête, changement d’URL et commandes
de redémarrage. Le smoke test CLI n’exerce pas ces fonctions graphiques.

## Référence des bases Zorin

[Détails techniques officiels Zorin OS](https://zorin.com/os/details/)
