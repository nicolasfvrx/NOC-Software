# Builds automatiques GitHub Actions

Le workflow est dans `.github/workflows/build.yml`. Placer **la racine NOC** à la
racine du dépôt GitHub, avec `noc-manager/`, `noc-display/`, `noc-agent/`, `scripts/`
et `doc/`. Les fichiers `Cargo.lock` doivent être versionnés.

Déclenchements : push d’un tag `v*` uniquement, ou lancement manuel via
**Actions → Build NOC applications → Run workflow**. Un push sur une branche
(y compris `main`) ou une pull request ne déclenche aucun build.

## Matrice et résultats

| Application | Compilation sur | Archive cible |
| --- | --- | --- |
| NOC Manager | Windows 2022, Rust 1.77.2 | Windows Server 2012 x64 |
| NOC Manager | Windows 2022, Rust 1.77.2 | Windows Server 2016 x64 |
| NOC Display | Windows 2022, Rust 1.77.2 | Windows Server 2012 x64 |
| NOC Display | Windows 2022, Rust 1.77.2 | Windows Server 2016 x64 |
| NOC Agent | Ubuntu 22.04, Rust stable | Ubuntu 22.04 / Zorin OS 17 x64 |
| NOC Agent | Ubuntu 24.04, Rust stable | Ubuntu 24.04 / Zorin OS 18 x64 |

Ouvrir une exécution terminée, puis **Artifacts** pour télécharger le résultat.
GitHub fournit un conteneur ZIP contenant le paquet `.zip` Windows ou `.tar.gz`
Linux. Les artifacts sont conservés 30 jours. Le paquet Linux est en tar.gz pour
conserver les permissions d’exécution malgré l’enveloppe de téléchargement GitHub.

Le workflow utilise les mêmes scripts que les builds locaux. Une erreur de
compilation, de métadonnées Windows, d’imports contrôlés, de version CLI, de
validation du lanceur Linux ou une bibliothèque Linux manquante fait échouer le job.
Les autres entrées de la matrice continuent pour faciliter le diagnostic.

## Récapitulatif de chaque exécution

Le job **NOC build summary** s’exécute après les builds et la publication, même
si un build échoue ou si la release est ignorée. Son résumé apparaît directement
sur la page de l’exécution GitHub Actions. Il contient :

- le commit, la branche ou le tag et le numéro de tentative ;
- les six cibles, le résultat des builds/contrôles/paquets et celui du job complet ;
- les archives disponibles, leur taille et leurs liens de téléchargement ;
- les étapes en échec, annulées ou non exécutées, avec accès aux logs ;
- la release publiée ou la raison pour laquelle elle n’a pas été publiée.

Un build réussi suivi d’un échec d’envoi est distingué d’un échec de compilation.
Les builds absents ne sont jamais présentés comme réussis. En cas de relance, le
résumé utilise le dernier résultat connu de chaque job, y compris les jobs réussis
d’une tentative précédente. Si GitHub interrompt tout le workflow avant de lancer
le résumé, celui-ci peut ne pas être généré.

## Releases automatiques

Un push d’un tag `v1.0.0` déclenche les six builds, puis crée automatiquement une
release **NOC v1.0.0** contenant les six archives d’installation et les quatre
exécutables bruts utilisés par la mise à jour automatique. Aucune release à créer
au préalable. Un tag avec suffixe, par exemple `v1.1.0-rc.1`, produit une préversion
GitHub. Utiliser le format `vMAJEUR.MINEUR.CORRECTIF`, avec un suffixe facultatif.

La publication attend la réussite de **tous** les builds Windows et Linux. Le job
crée d’abord un brouillon avec les notes générées par GitHub, ajoute les dix
paquets, puis publie. Une erreur d’envoi laisse le brouillon à reprendre en
relançant le job. Une release déjà publiée est conservée telle quelle lors d’une
relance : aucune archive publiée n’est remplacée. Pour corriger une version
publiée, créer un nouveau tag.

Un lancement manuel (`workflow_dispatch`) produit uniquement les artifacts, sans
publier de release. Pour reprendre une publication interrompue, utiliser **Re-run
failed jobs** sur l’exécution déclenchée par le tag. Si les artifacts ont expiré,
relancer tous les jobs de cette exécution.

Seul le job de publication reçoit `contents: write`. Il utilise le `GITHUB_TOKEN`
fourni par GitHub, sans PAT à configurer. Les jobs de build restent en lecture seule.
Les archives de release ne sont pas soumises à la conservation de 30 jours des
artifacts Actions. Le workflow n’installe rien sur les serveurs.

Voir le [guide des commits, pushes et tags](push-release.md) pour les commandes.

## Limites de validation

Les runners hébergés ne sont pas des Windows Server 2012/2016 ni des Zorin OS.
La compilation et les contrôles automatiques doivent être complétés par les
essais sur les systèmes cibles décrits dans les guides Windows et Linux.

Le workflow sera actif une fois ces fichiers poussés dans un dépôt GitHub avec
Actions activé. Sa présence locale ne constitue pas une exécution CI réussie.

Références : [checkout](https://github.com/actions/checkout),
[upload-artifact](https://github.com/actions/upload-artifact),
[création de release avec GitHub CLI](https://cli.github.com/manual/gh_release_create).
