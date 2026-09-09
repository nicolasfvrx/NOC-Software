//! Machine a etats du kiosque + libelles affiches a l'utilisateur.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KioskState {
    Starting,
    ConnectingToManager,
    FetchingConfig,
    ConfigNotFound,
    Disabled,
    CheckingTarget,
    TargetUnavailable,
    StartingFirefox,
    WaitingFirefox,
    LoadingPage,
    LoadingSlow,
    Running,
    Restarting,
    FirefoxCrashed,
    PageTimeout,
    ManagerUnavailable,
    /// geckodriver / wrapper Flatpak absent ou inutilisable.
    FirefoxComponentMissing,
    /// Commande distante `restart_browser` recue depuis NOC Manager.
    RemoteRestartBrowser,
    /// Commande distante `restart_agent` recue depuis NOC Manager.
    RemoteRestartAgent,
}

impl KioskState {
    /// Code court utilise dans kiosk-agent.log.
    pub fn code(self) -> &'static str {
        use KioskState::*;
        match self {
            Starting => "STARTING",
            ConnectingToManager => "CONNECTING_TO_MANAGER",
            FetchingConfig => "FETCHING_CONFIG",
            ConfigNotFound => "CONFIG_NOT_FOUND",
            Disabled => "DISABLED",
            CheckingTarget => "CHECKING_TARGET",
            TargetUnavailable => "TARGET_UNAVAILABLE",
            StartingFirefox => "STARTING_FIREFOX",
            WaitingFirefox => "WAITING_FIREFOX",
            LoadingPage => "LOADING_PAGE",
            LoadingSlow => "LOADING_SLOW",
            Running => "RUNNING",
            Restarting => "RESTARTING",
            FirefoxCrashed => "FIREFOX_CRASHED",
            PageTimeout => "PAGE_TIMEOUT",
            ManagerUnavailable => "MANAGER_UNAVAILABLE",
            FirefoxComponentMissing => "FIREFOX_COMPONENT_MISSING",
            RemoteRestartBrowser => "REMOTE_RESTART_BROWSER",
            RemoteRestartAgent => "REMOTE_RESTART_AGENT",
        }
    }

    /// Message principal (gros texte sous le logo).
    pub fn primary(self) -> &'static str {
        use KioskState::*;
        match self {
            Starting => "Initialisation du kiosque…",
            ConnectingToManager => "Connexion au serveur de configuration…",
            FetchingConfig => "Récupération de la configuration…",
            ConfigNotFound => "Kiosque non configuré",
            Disabled => "Affichage désactivé",
            CheckingTarget => "Vérification du service…",
            TargetUnavailable => "Service indisponible",
            StartingFirefox => "Démarrage de Firefox…",
            WaitingFirefox => "Préparation du navigateur…",
            LoadingPage => "La page est en cours de chargement…",
            LoadingSlow => "Le chargement prend plus de temps que prévu…",
            Running => "Affichage en cours",
            Restarting => "Actualisation de l'affichage…",
            FirefoxCrashed => "Firefox s'est arrêté",
            PageTimeout => "La page ne répond pas correctement",
            ManagerUnavailable => "Serveur de configuration indisponible",
            FirefoxComponentMissing => "Composant Firefox indisponible",
            RemoteRestartBrowser => "Actualisation de l'affichage…",
            RemoteRestartAgent => "Redémarrage de l'agent…",
        }
    }

    /// Message secondaire par defaut. `{username}` est substitue a l'affichage.
    pub fn secondary(self) -> &'static str {
        use KioskState::*;
        match self {
            Starting => "Préparation de l'affichage",
            ConnectingToManager => "Contact du serveur NOC Manager",
            FetchingConfig => "Lecture des paramètres du kiosque",
            ConfigNotFound => "Aucune configuration n'existe pour {username}",
            Disabled => "Ce kiosque a été désactivé depuis le serveur",
            CheckingTarget => "Contrôle de l'accessibilité de la destination",
            TargetUnavailable => "Impossible de joindre le serveur",
            StartingFirefox => "Lancement du navigateur en arrière-plan",
            WaitingFirefox => "Initialisation de la session WebDriver",
            LoadingPage => "Le navigateur travaille en arrière-plan",
            LoadingSlow => "Toujours en attente de la page…",
            Running => "",
            Restarting => "Redémarrage planifié du navigateur",
            FirefoxCrashed => "Redémarrage du navigateur…",
            PageTimeout => "Le navigateur va être relancé",
            ManagerUnavailable => "Impossible de contacter NOC Manager",
            FirefoxComponentMissing => "geckodriver ou le wrapper Flatpak est introuvable",
            RemoteRestartBrowser => "Redémarrage du navigateur",
            RemoteRestartAgent => "Réinitialisation de l'affichage",
        }
    }
}

/// Messages progressifs pendant le chargement de la page.
/// (secondes ecoulees minimum, message). Facile a modifier.
pub const LOADING_STEPS: &[(u64, &str)] = &[
    (0, "La page est en cours de chargement…"),
    (5, "Chargement du tableau de bord…"),
    (15, "Le chargement prend plus de temps que prévu…"),
    (30, "Toujours en attente de la page…"),
    (45, "Finalisation de l'affichage…"),
];

pub fn loading_message(elapsed_seconds: u64) -> &'static str {
    let mut current = LOADING_STEPS[0].1;
    for (after, msg) in LOADING_STEPS {
        if elapsed_seconds >= *after {
            current = msg;
        }
    }
    current
}
