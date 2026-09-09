//! Visual treatment of an application state; orchestration lives in state.rs/app.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ready,
    Loading,
    ServerUnavailable,
    SessionEnded,
}
impl Status {
    pub fn message(self) -> &'static str {
        match self {
            Self::Ready => "Bienvenue. Votre poste est prêt.",
            Self::Loading => "Connexion en cours…",
            Self::ServerUnavailable => "Connexion impossible",
            Self::SessionEnded => "Connexion interrompue",
        }
    }
    pub fn is_loading(self) -> bool {
        self == Self::Loading
    }
    pub fn is_error(self) -> bool {
        !matches!(self, Self::Ready | Self::Loading)
    }
}
