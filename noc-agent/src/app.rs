//! Orchestrateur : machine a etats complete du kiosque.
//! Tourne dans un runtime tokio, sur un thread separe du thread UI.

use crate::commands::{self, PendingAction, RemoteControl};
use crate::config::Config;
use crate::logging;
use crate::manager::{self, FetchOutcome, KioskConfig, TargetCheck};
use crate::scheduler::CronRestart;
use crate::state::{loading_message, KioskState};
use crate::ui::UiHandle;
use crate::webdriver::{Browser, LaunchError, PageError};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Temps laisse a la fenetre de statut pour reapparaitre et passer devant
/// Firefox avant qu'on arrete ce dernier : sans cela, le bureau XFCE et sa
/// barre de taches apparaissent brievement.
const UI_SETTLE: Duration = Duration::from_millis(700);

/// Raison pour laquelle on quitte l'etat RUNNING.
enum Outcome {
    FirefoxGone(String),
    Disabled,
    ConfigChanged(String),
    CronRestart,
    /// Commande distante recue pendant l'affichage.
    Remote(PendingAction),
}

/// Resultat de l'attente de page prete.
enum LoadOutcome {
    Ready,
    /// Chargement annule par une commande distante prioritaire.
    Interrupted(PendingAction),
}

pub async fn run(cfg: Config, base: PathBuf, username: String, ui: UiHandle) {
    let manager_client = manager::build_client(cfg.manager.timeout_seconds, false);
    let target_client = manager::build_client(cfg.target.timeout_seconds, cfg.target.insecure);
    let mut cron = CronRestart::new();
    let remote = Arc::new(RemoteControl::new());

    // Polling leger des commandes distantes, greffe sur le fonctionnement
    // normal : il alimente simplement la file `remote`.
    tokio::spawn(commands::poll_loop(
        cfg.clone(),
        username.clone(),
        ui.clone(),
        remote.clone(),
    ));

    logging::log(
        "STARTING",
        &format!(
            "kiosque={username} manager={} poll={}s",
            cfg.manager.url, cfg.manager.poll_seconds
        ),
    );
    ui.set(KioskState::Starting);
    sleep(Duration::from_millis(600)).await;

    loop {
        // L'ecran de statut est visible dans tous les etats d'attente.
        ui.show();

        // ---------------------------------------------------- configuration
        ui.set(KioskState::ConnectingToManager);
        ui.set_detail(&cfg.manager.url);
        sleep(Duration::from_millis(300)).await;
        ui.set(KioskState::FetchingConfig);
        ui.set_detail(&format!(
            "GET {}/api/kiosk/{username}",
            cfg.manager.url.trim_end_matches('/')
        ));

        let kiosk = match manager::fetch_kiosk(&manager_client, &cfg.manager.url, &username).await {
            FetchOutcome::Unavailable(reason) => {
                ui.set_with(KioskState::ManagerUnavailable, Some(&reason));
                countdown(&ui, cfg.manager.retry_seconds, &remote, &cfg).await;
                continue;
            }
            FetchOutcome::NotFound => {
                ui.set(KioskState::ConfigNotFound);
                countdown(&ui, cfg.manager.retry_seconds, &remote, &cfg).await;
                continue;
            }
            FetchOutcome::Found(kiosk) => *kiosk,
        };

        if cron.update(kiosk.restart_cron.as_deref()) {
            log_cron(&cron);
        }

        if !kiosk.enabled {
            ui.set(KioskState::Disabled);
            countdown(&ui, cfg.manager.poll_seconds, &remote, &cfg).await;
            continue;
        }

        // ------------------------------------------------------ test de l'URL
        ui.set(KioskState::CheckingTarget);
        ui.set_detail(kiosk.label());
        match manager::check_target(&target_client, &kiosk.url).await {
            TargetCheck::Reachable(code) => {
                logging::log("CHECKING_TARGET", &format!("HTTP {code}"));
            }
            TargetCheck::Unusable(reason) => {
                // On ne montre JAMAIS Firefox sur une 404 / 5xx.
                ui.set_with(KioskState::TargetUnavailable, Some(&reason));
                countdown(&ui, cfg.manager.retry_seconds, &remote, &cfg).await;
                continue;
            }
        }

        // ------------------------------------------------------- Firefox
        if remote.browser_restart_in_progress() {
            logging::log("BROWSER_RESTART", "starting firefox");
        }
        ui.set(KioskState::StartingFirefox);
        ui.set_detail("Flatpak org.mozilla.firefox + geckodriver");
        let mut browser = match Browser::launch(&cfg, &base).await {
            Ok(browser) => browser,
            Err(LaunchError::MissingComponent(reason)) => {
                logging::log("FIREFOX_COMPONENT_MISSING", &reason);
                ui.set_with(KioskState::FirefoxComponentMissing, Some(&reason));
                countdown(&ui, cfg.firefox.retry_seconds.max(10), &remote, &cfg).await;
                continue;
            }
            Err(LaunchError::Other(reason)) => {
                logging::log("STARTING_FIREFOX", &format!("échec : {reason}"));
                ui.set_with(KioskState::FirefoxCrashed, Some(&reason));
                countdown(&ui, cfg.firefox.retry_seconds, &remote, &cfg).await;
                continue;
            }
        };

        // -------------------------------------------- navigation + attente
        ui.set(KioskState::WaitingFirefox);
        match load_page(&ui, &browser, &kiosk, &cfg, &remote).await {
            Ok(LoadOutcome::Ready) => {}
            Ok(LoadOutcome::Interrupted(PendingAction::RestartAgent)) => {
                cleanup_for_agent_restart(Some(browser), &cfg, &ui).await;
                restart_self();
            }
            Ok(LoadOutcome::Interrupted(PendingAction::RestartBrowser)) => {
                // Le chargement en cours est annule proprement.
                logging::log("BROWSER_RESTART", "begin source=remote (chargement annule)");
                remote.begin_browser_restart();
                ui.show();
                ui.set(KioskState::RemoteRestartBrowser);
                sleep(UI_SETTLE).await;
                browser.shutdown(&cfg).await;
                logging::log("BROWSER_RESTART", "firefox stopped");
                sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(PageError::Timeout) => {
                logging::log("PAGE_TIMEOUT", "page non prête dans le délai imparti");
                ui.set(KioskState::PageTimeout);
                browser.shutdown(&cfg).await;
                countdown(&ui, cfg.firefox.retry_seconds, &remote, &cfg).await;
                continue;
            }
            Err(PageError::SessionLost(reason)) => {
                logging::log("FIREFOX_CRASHED", &reason);
                ui.set(KioskState::FirefoxCrashed);
                browser.shutdown(&cfg).await;
                countdown(&ui, cfg.firefox.retry_seconds, &remote, &cfg).await;
                continue;
            }
        }

        // ------------------------------------------------------- RUNNING
        logging::log("RUNNING", "page ready");
        if remote.browser_restart_in_progress() {
            logging::log("BROWSER_RESTART", "page ready");
            remote.end_browser_restart();
        }
        if let Some(url) = browser.current_url().await {
            logging::log("RUNNING", &format!("URL affichée : {url}"));
        }
        ui.set(KioskState::Running);
        ui.set_detail(&kiosk.url);
        // Seulement maintenant : NOC Agent se masque, Firefox apparait.
        ui.hide();

        let outcome = supervise(
            &mut browser,
            &kiosk,
            &cfg,
            &manager_client,
            &mut cron,
            &username,
            &remote,
        )
        .await;

        // Des que quelque chose change : NOC Agent revient immediatement
        // au premier plan, avant meme d'arreter Firefox.
        ui.show();
        // restart_agent est prioritaire sur tout le reste.
        if let Outcome::Remote(PendingAction::RestartAgent) = &outcome {
            cleanup_for_agent_restart(Some(browser), &cfg, &ui).await;
            restart_self();
        }
        match &outcome {
            Outcome::FirefoxGone(reason) => {
                logging::log("FIREFOX_CRASHED", reason);
                ui.set(KioskState::FirefoxCrashed);
            }
            Outcome::Disabled => {
                ui.set(KioskState::Disabled);
            }
            Outcome::ConfigChanged(detail) => {
                logging::log("RESTARTING", detail);
                ui.set_with(KioskState::Restarting, Some("Mise à jour de l'affichage…"));
            }
            Outcome::CronRestart => {
                logging::log("RESTARTING", "cron triggered");
                ui.set(KioskState::Restarting);
            }
            Outcome::Remote(_) => {
                logging::log("BROWSER_RESTART", "begin source=remote");
                remote.begin_browser_restart();
                ui.set(KioskState::RemoteRestartBrowser);
            }
        }

        ui.set_detail("Arrêt du navigateur…");
        // L'ecran de statut doit etre devant AVANT que Firefox disparaisse.
        sleep(UI_SETTLE).await;
        let remote_restart = remote.browser_restart_in_progress();
        browser.shutdown(&cfg).await;
        if remote_restart {
            logging::log("BROWSER_RESTART", "firefox stopped");
        }
        let delay = if remote_restart {
            2
        } else {
            cfg.firefox.restart_delay_seconds
        };
        sleep(Duration::from_secs(delay)).await;
    }
}

/// Navigue puis attend la VRAIE fin de chargement (readyState + selecteur).
async fn load_page(
    ui: &UiHandle,
    browser: &Browser,
    kiosk: &KioskConfig,
    cfg: &Config,
    remote: &RemoteControl,
) -> Result<LoadOutcome, PageError> {
    let selector = kiosk.ready_selector().to_string();
    logging::log(
        "LOADING_PAGE",
        &format!(
            "navigation vers la destination (ready_selector={})",
            if selector.is_empty() { "aucun" } else { &selector }
        ),
    );

    browser.goto(&kiosk.url).await?;

    let started = Instant::now();
    let timeout = cfg.firefox.load_timeout_seconds.max(5);
    let slow_after = cfg.firefox.slow_loading_after_seconds;
    let mut last_logged_state = String::new();

    loop {
        // Une commande distante est prioritaire sur le chargement en cours.
        if let Some(action) = remote.take() {
            return Ok(LoadOutcome::Interrupted(action));
        }

        let elapsed = started.elapsed().as_secs();
        if elapsed >= timeout {
            return Err(PageError::Timeout);
        }

        let message = loading_message(elapsed);
        let state = if elapsed >= slow_after {
            KioskState::LoadingSlow
        } else {
            KioskState::LoadingPage
        };
        ui.set_with(state, Some(message));
        ui.set_detail(&format!("{elapsed} s / {timeout} s"));

        let (ready, ready_state) = browser.probe_ready(&selector).await?;
        if ready_state != last_logged_state {
            logging::log("LOADING_PAGE", &format!("readyState={ready_state}"));
            last_logged_state = ready_state;
        }
        if ready {
            return Ok(LoadOutcome::Ready);
        }

        sleep(Duration::from_millis(500)).await;
    }
}

/// Etat RUNNING : NOC Agent est masque, on surveille Firefox, le cron
/// et la configuration cote NOC Manager.
async fn supervise(
    browser: &mut Browser,
    kiosk: &KioskConfig,
    cfg: &Config,
    manager_client: &reqwest::Client,
    cron: &mut CronRestart,
    username: &str,
    remote: &RemoteControl,
) -> Outcome {
    let mut seconds_since_poll = 0u64;
    let poll_every = cfg.manager.poll_seconds.max(5);

    loop {
        sleep(Duration::from_secs(1)).await;
        seconds_since_poll += 1;

        // 0. Commande distante : priorite sur tout le reste.
        if let Some(action) = remote.take() {
            return Outcome::Remote(action);
        }

        // 1. Firefox est-il toujours la ?
        if !browser.is_alive().await {
            return Outcome::FirefoxGone("session Firefox perdue (fermeture ou crash)".to_string());
        }

        // 2. Redemarrage planifie ?
        if cron.due() {
            return Outcome::CronRestart;
        }

        // 3. Relecture periodique de la configuration.
        if seconds_since_poll >= poll_every {
            seconds_since_poll = 0;
            match manager::fetch_kiosk(manager_client, &cfg.manager.url, username).await {
                FetchOutcome::Found(latest) => {
                    if !latest.enabled {
                        return Outcome::Disabled;
                    }
                    if latest.url != kiosk.url {
                        return Outcome::ConfigChanged("URL modifiée côté manager".to_string());
                    }
                    if latest.ready_selector() != kiosk.ready_selector() {
                        return Outcome::ConfigChanged(
                            "ready_selector modifié côté manager".to_string(),
                        );
                    }
                    // Nouvelle expression cron : prise en compte a chaud,
                    // sans redemarrer l'agent ni Firefox.
                    if cron.update(latest.restart_cron.as_deref()) {
                        log_cron(cron);
                    }
                }
                FetchOutcome::NotFound => {
                    logging::log(
                        "MANAGER_POLL",
                        "kiosque absent du manager, affichage conservé",
                    );
                }
                FetchOutcome::Unavailable(reason) => {
                    // Manager injoignable : on NE coupe PAS l'affichage en cours.
                    logging::log(
                        "MANAGER_UNAVAILABLE",
                        &format!("{reason} (affichage conservé)"),
                    );
                }
            }
        }
    }
}

/// Compte a rebours affiche sur la troisieme ligne de l'ecran de statut.
async fn countdown(ui: &UiHandle, seconds: u64, remote: &RemoteControl, cfg: &Config) {
    // Etat d'attente : aucun navigateur n'est lance, un nouveau
    // restart_browser distant redevient donc acceptable.
    remote.end_browser_restart();

    let total = seconds.max(1);
    for remaining in (1..=total).rev() {
        match remote.take() {
            Some(PendingAction::RestartAgent) => {
                cleanup_for_agent_restart(None, cfg, ui).await;
                restart_self();
            }
            Some(PendingAction::RestartBrowser) => {
                // Aucun navigateur a arreter : on repart immediatement sur
                // le cycle normal config -> test URL -> Firefox.
                logging::log("BROWSER_RESTART", "begin source=remote (aucun navigateur actif)");
                return;
            }
            None => {}
        }

        let plural = if remaining > 1 { "s" } else { "" };
        ui.set_detail(&format!(
            "Nouvelle tentative dans {remaining} seconde{plural}"
        ));
        sleep(Duration::from_secs(1)).await;
    }
    ui.set_detail("Nouvelle tentative…");
}

fn log_cron(cron: &CronRestart) {
    if cron.expression().is_empty() {
        logging::log("CRON", "aucun redémarrage planifié");
    } else if cron.is_invalid() {
        logging::log(
            "CRON",
            &format!("expression illisible, ignorée : {}", cron.expression()),
        );
    } else {
        let next = cron
            .next_run()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "jamais".to_string());
        logging::log(
            "CRON",
            &format!("planification \"{}\" — prochain : {next}", cron.expression()),
        );
    }
}

/// Arret propre avant un redemarrage complet de l'agent par systemd.
async fn cleanup_for_agent_restart(browser: Option<Browser>, cfg: &Config, ui: &UiHandle) {
    logging::log("AGENT_RESTART", "cleanup begin");
    match browser {
        Some(browser) => {
            ui.set_detail("Arrêt du navigateur…");
            sleep(UI_SETTLE).await;
            browser.shutdown(cfg).await;
        }
        None => logging::log("AGENT_RESTART", "aucun navigateur a arreter"),
    }
    // Laisse le temps a la fenetre d'afficher "Redemarrage de l'agent...".
    sleep(Duration::from_millis(500)).await;
}

/// NOC Agent se relance lui-meme via exec (memes PID/argv/environnement) :
/// ca fonctionne aussi bien sous le service systemd (Restart=always, qui ne
/// voit alors aucun arret a relancer) que sous l'autostart XFCE, qui ne
/// relance jamais un processus termine.
fn restart_self() -> ! {
    // Le processus relance n'a pas besoin de revérifier les mises à jour.
    std::env::set_var("NOC_SKIP_UPDATE", "noc-agent");
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            logging::log("AGENT_RESTART", &format!("current_exe indisponible : {e}, exit(0)"));
            std::process::exit(0);
        }
    };
    logging::log("AGENT_RESTART", &format!("re-exec {}", exe.display()));
    let error = std::process::Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .exec();
    logging::log("AGENT_RESTART", &format!("exec a échoué : {error}, exit(0)"));
    std::process::exit(0);
}
