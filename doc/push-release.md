# Envoyer le code et publier une version

Exemples à exécuter dans PowerShell, à la racine du projet :

```powershell
cd C:/Users/Nicolas/Projets/noc
```

La branche utilisée dans ce guide est `main`. Adapter ce nom si le dépôt utilise
une autre branche. Git et une authentification GitHub doivent être disponibles
(Git Credential Manager avec HTTPS, ou une clé SSH).

## Premier envoi — une seule fois

Si NOC n’est pas encore un dépôt Git, créer un dépôt **vide** sur GitHub, sans
README, licence ni `.gitignore` générés, puis exécuter :

```powershell
git init -b main
git add .
git diff --cached --stat
git commit -m "Initialise NOC applications and automated builds"
git remote add origin git@github.com:nicolasfvrx/NOC-Software.git
git push -u origin main
```

Le dépôt est [nicolasfvrx/NOC-Software](https://github.com/nicolasfvrx/NOC-Software).
L’URL ci-dessus utilise SSH : la clé du poste doit être autorisée sur GitHub.
Si Git demande une identité,
configurer `git config user.name "Ton nom"` et `git config user.email "Ton adresse"`,
puis reprendre le commit. Si le dépôt existe déjà, ne pas refaire l’initialisation :
vérifier `git status`, `git branch --show-current` et `git remote -v`.

Les `.gitignore` excluent les builds, l’outillage local et les configurations
locales. Examiner les fichiers indexés avant chaque commit. Les `Cargo.lock`, les
exemples de configuration, les scripts et `.github/workflows/build.yml` doivent
bien être inclus.

## Envoyer des modifications courantes

```powershell
git status
git add .
git diff --cached --stat
git diff --cached
git commit -m "Describe the changes"
git push origin main
```

Un push sur `main` ne déclenche aucun build : seuls les tags `v*` et le lancement
manuel (**Actions → Build NOC applications → Run workflow**) le font. Pour vérifier
les builds avant de taguer, lancer le workflow manuellement sur `main`. Si la
branche est protégée, pousser une branche de travail et passer par une pull
request. Si un push est rejeté car le dépôt a avancé, récupérer et intégrer les
modifications avant de réessayer ; ne pas forcer le push pour contourner ce rejet.

## Publier une release

Après avoir envoyé le code, et éventuellement vérifié les builds de `main` via un
lancement manuel :

```powershell
git switch main
git pull --ff-only origin main
git status
git log -1 --oneline
git tag -a v1.0.0 -m "NOC v1.0.0"
git push origin v1.0.0
```

Vérifier avant le tag que le dossier de travail est propre et que le dernier
commit est bien celui à publier. Le tag désigne ce commit précis ; il n’inclut
jamais les modifications non commitées. Pousser seulement le tag voulu avec la
commande ci-dessus, plutôt que tous les tags locaux.

Dans **Actions → Build NOC applications**, ouvrir l’exécution du tag. Lorsque les
six builds et le job **Publish GitHub release** réussissent, la page **Releases**
contient **NOC v1.0.0**, les notes automatiques, les six archives d’installation
et les quatre exécutables bruts utilisés par la mise à jour automatique.
Ne pas créer la release à la main avant l’exécution.

Pour la version suivante, utiliser un nouveau tag, par exemple :

```powershell
git tag -a v1.0.1 -m "NOC v1.0.1"
git push origin v1.0.1
```

Pour une préversion :

```powershell
git tag -a v1.1.0-rc.1 -m "NOC v1.1.0 release candidate 1"
git push origin v1.1.0-rc.1
```

## Tag de suite et versions des applications

Le tag identifie une livraison de l’ensemble **NOC**. Avant chaque release,
aligner `VERSION`, le champ `version` des trois `Cargo.toml` et les entrées des
applications dans leurs `Cargo.lock` sur la version du tag sans le préfixe `v`.
Utiliser Cargo avec Rust 1.77.2 pour les deux applications Windows afin de
conserver leur format de verrouillage, et Cargo stable pour Agent.

Exécuter `python scripts/check-versions.py` puis les vérifications de build,
et commiter les changements avant de créer le tag. La matrice CI refuse de
compiler si le tag, `VERSION` ou les versions Cargo ne correspondent pas.

## En cas d’échec

- Un build échoue : aucune release n’est publiée. Consulter les logs dans Actions.
- Un envoi d’archive échoue : la release reste en brouillon. Utiliser **Re-run failed
  jobs** sur l’exécution du tag pour reprendre l’envoi et publier.
- Les artifacts ont expiré : utiliser **Re-run all jobs** sur cette même exécution.
- La release est déjà publiée : la relance ne remplace aucun fichier. Faire une
  correction dans un nouveau commit et publier un nouveau tag.
- La publication est refusée : vérifier que GitHub Actions est activé et que les
  règles du dépôt ou de l’organisation autorisent le job avec `contents: write`.

Une correction des sources ou du workflow après création du tag doit être committée
et publiée sous **un nouveau tag** : relancer l’ancienne exécution utilise l’ancien
commit. Éviter de déplacer un tag déjà publié.
