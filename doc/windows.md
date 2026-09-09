# Build Windows — NOC Manager et NOC Display

## Plateformes

Les deux applications ciblent **Windows Server 2012 et Windows Server 2016 x64**.
La compilation se fait sur un poste Windows récent (Windows 10/11 ou un runner
GitHub `windows-2022`). Le système de compilation et le système de destination
sont distincts : GitHub ne démarre pas ici de machine Server 2012 ou 2016.

Les deux déclinaisons utilisent volontairement **Rust 1.77.2**, la même cible
`x86_64-pc-windows-msvc` et le CRT MSVC statique. Le suffixe de l’archive identifie
la destination ; il ne correspond pas à deux implémentations différentes.
NOC Display conserve le sous-système PE Windows 6.02. Ne pas substituer `stable`
à cette version : Rust a relevé le minimum Windows à partir de 1.78.

## Prérequis sur le poste de compilation

1. Installer Visual Studio 2022 Build Tools avec **Développement Desktop en C++**,
   les outils **MSVC x64/x86**, et un **Windows SDK** incluant `rc.exe`.
2. Installer Rust avec [rustup](https://rustup.rs/), puis ouvrir PowerShell :

```powershell
rustup toolchain install 1.77.2 --profile minimal --target x86_64-pc-windows-msvc
```

`cargo`, `rustc` et `rustup` doivent être dans le `PATH`. Les scripts utilisent
`vswhere.exe` pour trouver `dumpbin.exe`. Le build recherche `rc.exe` dans le SDK
Windows ; au besoin définir `$env:RC` avec son chemin complet.

## Compiler et créer les archives

Depuis la racine `noc` :

```powershell
./scripts/build-windows.ps1 -App noc-manager -Server 2012
./scripts/build-windows.ps1 -App noc-display -Server 2012
./scripts/build-windows.ps1 -App noc-manager -Server 2016
./scripts/build-windows.ps1 -App noc-display -Server 2016
```

Chaque commande compile en release, vérifie les métadonnées du fichier et
recherche quelques imports incompatibles avec la base commune (notamment
`bcryptprimitives.dll`, `ProcessPrng`, `SetThreadDescription` et le runtime MSVC
dynamique). Ce contrôle détecte des régressions connues ; ce n’est pas une preuve
exhaustive de compatibilité de toutes les API importées.

Les paquets incluent l’exécutable, les exemples de configuration, la documentation,
les images pour Display, un fichier `BUILD-INFO.txt` et l’empreinte SHA-256 du binaire.

```text
dist/
  noc-manager-windows-server-2012-x64.zip
  noc-display-windows-server-2012-x64.zip
  noc-manager-windows-server-2016-x64.zip
  noc-display-windows-server-2016-x64.zip
```

Le script utilise un dossier temporaire neuf à chaque exécution. Il refuse les
surcharges de flags Rust par variables d’environnement pour éviter de perdre les
réglages de compatibilité du projet.

## Compiler uniquement un exécutable

```powershell
cd noc-manager
cargo +1.77.2 build --release --locked --target x86_64-pc-windows-msvc
cd ../noc-display
cargo +1.77.2 build --release --locked --target x86_64-pc-windows-msvc
```

Le résultat est dans `target/x86_64-pc-windows-msvc/release/` de chaque projet.
Exécuter Cargo **depuis le dossier de l’application** pour qu’il lise son
`.cargo/config.toml`. Un simple `--manifest-path` depuis la racine ne charge pas
automatiquement cette configuration.

## Validation et mise en service

Sur chaque version de Windows Server cible, vérifier :

- Manager : `noc-manager.exe --version`, démarrage du serveur, interface web et
  route `/api/health` (avec le token si activé).
- Display : démarrage dans une session Windows graphique, images, connexion RDP,
  retour à l’écran de statut après coupure, puis reconnexion.

Pour Manager, copier `config.example.toml` en `config.toml` dans son dossier de
travail. Pour Display, copier l’exemple dans `%USERPROFILE%/config.toml` pour chaque
compte d’affichage et placer les images à côté de l’exécutable. Adapter les
raccourcis, tâches planifiées et le shell personnalisé au nouveau nom du binaire.
NOC Display nécessite un bureau graphique et le contrôle RDP `mstscax.dll` ; le
build ne transforme pas Server Core en poste d’affichage.

## Références

- [Changement du minimum Windows dans Rust 1.78](https://blog.rust-lang.org/2024/02/26/Windows-7/)
- [Cible Windows MSVC actuelle](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html)
- [Outils du runner Windows 2022](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md)
