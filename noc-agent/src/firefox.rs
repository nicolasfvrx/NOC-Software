//! Localisation de geckodriver / du wrapper Flatpak, profil persistant,
//! et arret force cible en dernier recours.

use crate::config::{expand_tilde, FirefoxConfig};
use crate::logging;
use std::path::{Path, PathBuf};

/// Recherche un exécutable dans $PATH (equivalent de `which`).
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Chemin de geckodriver : valeur de config, sinon $PATH,
/// sinon quelques emplacements habituels.
pub fn resolve_geckodriver(cfg: &FirefoxConfig) -> Option<PathBuf> {
    if !cfg.geckodriver.trim().is_empty() {
        let p = expand_tilde(cfg.geckodriver.trim());
        return p.is_file().then_some(p);
    }
    if let Some(p) = which("geckodriver") {
        return Some(p);
    }
    for fallback in [
        "/usr/bin/geckodriver",
        "/usr/local/bin/geckodriver",
        "/snap/bin/geckodriver",
    ] {
        let p = PathBuf::from(fallback);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Wrapper passe a geckodriver comme binaire Firefox.
/// On ne pointe JAMAIS vers /usr/bin/firefox (inexistant avec Flatpak).
pub fn resolve_wrapper(cfg: &FirefoxConfig, base: &Path) -> Result<PathBuf, String> {
    let candidate = if cfg.wrapper.trim().is_empty() {
        base.join("firefox-flatpak-wrapper.sh")
    } else {
        let p = expand_tilde(cfg.wrapper.trim());
        if p.is_absolute() {
            p
        } else {
            base.join(p)
        }
    };

    if !candidate.is_file() {
        return Err(format!(
            "wrapper Flatpak introuvable : {}",
            candidate.display()
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.permissions().mode() & 0o111 == 0 => {
                return Err(format!(
                    "wrapper non exécutable : chmod +x {}",
                    candidate.display()
                ));
            }
            Err(e) => return Err(format!("wrapper illisible : {e}")),
            _ => {}
        }
    }

    Ok(candidate)
}

/// Profil Firefox persistant (cookies, session Grafana, certificats).
/// Doit rester dans un dossier accessible depuis le bac a sable Flatpak.
pub fn ensure_profile_dir(cfg: &FirefoxConfig) -> Result<PathBuf, String> {
    let dir = expand_tilde(&cfg.profile_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("profil Firefox impossible à créer ({}) : {e}", dir.display()))?;
    Ok(dir)
}

/// Chemin de la commande `flatpak`.
pub fn flatpak_bin(cfg: &FirefoxConfig) -> Option<String> {
    if Path::new(&cfg.flatpak).is_file() {
        return Some(cfg.flatpak.clone());
    }
    which("flatpak").map(|p| p.to_string_lossy().to_string())
}

/// Verifie que Firefox est bien installe via Flatpak.
pub fn flatpak_app_installed(cfg: &FirefoxConfig) -> bool {
    let flatpak = match flatpak_bin(cfg) {
        Some(p) => p,
        None => return false,
    };
    match std::process::Command::new(flatpak)
        .args(["info", &cfg.app_id])
        .output()
    {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Dernier recours : `flatpak kill <app_id>`.
/// Cette commande n'agit que sur les instances Flatpak de l'utilisateur
/// Linux courant — jamais sur les autres sessions kiosque.
/// On n'utilise volontairement PAS `pkill firefox`.
pub async fn flatpak_kill(cfg: &FirefoxConfig) {
    if !cfg.force_kill {
        return;
    }
    let flatpak = match flatpak_bin(cfg) {
        Some(p) => p,
        None => return,
    };
    logging::log(
        "FIREFOX_STOP",
        &format!("flatpak kill {} (session utilisateur uniquement)", cfg.app_id),
    );
    let _ = tokio::process::Command::new(flatpak)
        .args(["kill", &cfg.app_id])
        .status()
        .await;
}

/// Une instance Flatpak de Firefox tourne-t-elle pour l'utilisateur
/// courant ? (`flatpak ps` est scope a la session utilisateur.)
pub async fn instance_running(cfg: &FirefoxConfig) -> bool {
    let Some(flatpak) = flatpak_bin(cfg) else {
        return false;
    };
    match tokio::process::Command::new(flatpak)
        .args(["ps", "--columns=application"])
        .output()
        .await
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|line| line.trim() == cfg.app_id),
        Err(_) => false,
    }
}

/// Attend la disparition des instances Flatpak (au plus `seconds`).
async fn wait_until_gone(cfg: &FirefoxConfig, seconds: u64) -> bool {
    for _ in 0..seconds {
        if !instance_running(cfg).await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    !instance_running(cfg).await
}

/// Verrous laisses par un Firefox tue brutalement. Tant qu'ils sont la,
/// Firefox refuse de demarrer ("Firefox est deja en cours d'execution").
/// A n'appeler que si plus aucune instance ne tourne.
pub fn clear_profile_locks(profile: &Path) {
    for name in [".parentlock", "lock"] {
        let path = profile.join(name);
        if std::fs::symlink_metadata(&path).is_ok() && std::fs::remove_file(&path).is_ok() {
            logging::log(
                "FIREFOX_CLEANUP",
                &format!("verrou de profil supprimé : {}", path.display()),
            );
        }
    }
}

/// A appeler AVANT chaque lancement : aucune instance residuelle ne doit
/// subsister, sinon Firefox refuse de demarrer et geckodriver echoue.
/// `flatpak kill` reste limite aux instances de l'utilisateur Linux
/// courant : les autres sessions kiosque ne sont jamais touchees, et on
/// n'utilise toujours pas `pkill firefox`.
pub async fn ensure_no_leftover(cfg: &FirefoxConfig, profile: &Path) {
    if instance_running(cfg).await {
        logging::log(
            "FIREFOX_CLEANUP",
            &format!("instance {} résiduelle détectée avant lancement", cfg.app_id),
        );
        flatpak_kill(cfg).await;
        if !wait_until_gone(cfg, 10).await {
            logging::log(
                "FIREFOX_CLEANUP",
                "instance toujours présente après flatpak kill",
            );
            return;
        }
        logging::log("FIREFOX_CLEANUP", "instance résiduelle arrêtée");
    }
    clear_profile_locks(profile);
}

/// A appeler APRES l'arret : le kill de geckodriver ne tue que le wrapper
/// `flatpak run`, pas toujours l'application dans son bac a sable.
pub async fn ensure_stopped(cfg: &FirefoxConfig) {
    if wait_until_gone(cfg, 5).await {
        return;
    }
    logging::log(
        "FIREFOX_STOP",
        "Firefox toujours actif après l'arrêt du driver",
    );
    flatpak_kill(cfg).await;
    if wait_until_gone(cfg, 10).await {
        logging::log("FIREFOX_STOP", "Firefox arrêté");
    } else {
        logging::log("FIREFOX_STOP", "Firefox n'a pas pu être arrêté");
    }
}
