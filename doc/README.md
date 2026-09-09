# Compiler les applications NOC

| Application | Plateforme cible | Guide |
| --- | --- | --- |
| NOC Manager | Windows Server 2012 et 2016, x64 | [Windows](windows.md) |
| NOC Display | Windows Server 2012 et 2016 avec interface graphique, x64 | [Windows](windows.md) |
| NOC Agent | Ubuntu 22.04 / 24.04, Zorin OS 17 / 18, x64 | [Linux](linux.md) |

Les projets sont indépendants et possèdent chacun un `Cargo.lock`. Compiler avec
`--locked` pour conserver les versions des dépendances. Les archives produites
sont placées dans `dist/`, ignoré par Git.

Voir [GitHub Actions](github-actions.md) pour les builds automatiques et le
téléchargement des archives. Les exemples de configuration sont livrés ; les
configurations locales, journaux, identifiants et profils utilisateurs ne sont
pas inclus dans les paquets.

Voir le [guide des pushes et releases](push-release.md) pour le premier envoi sur
GitHub, les mises à jour et la publication automatique par tag.
