use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Starting,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}
impl AppState {
    /// Court libelle envoye dans le heartbeat vers NOC Manager.
    pub fn code(self) -> &'static str {
        match self {
            Self::Starting => "STARTING",
            Self::Connecting => "CONNECTING",
            Self::Connected => "CONNECTED",
            Self::Reconnecting => "RECONNECTING",
            Self::Error => "ERROR",
        }
    }
}
pub struct Machine {
    pub state: AppState,
    pub retry_at: Option<Instant>,
    pub deadline: Option<Instant>,
    pub ever_connected: bool,
}
impl Machine {
    pub fn new() -> Self {
        Self {
            state: AppState::Starting,
            retry_at: None,
            deadline: None,
            ever_connected: false,
        }
    }
    pub fn connecting(&mut self, now: Instant) {
        self.state = AppState::Connecting;
        self.retry_at = None;
        self.deadline = Some(now + Duration::from_secs(45));
    }
    pub fn login_complete(&mut self) -> bool {
        if self.state != AppState::Connecting {
            return false;
        }
        self.state = AppState::Connected;
        self.ever_connected = true;
        self.deadline = None;
        true
    }
    pub fn failure(&mut self, now: Instant, delay: u32) {
        self.state = if self.ever_connected {
            AppState::Reconnecting
        } else {
            AppState::Error
        };
        self.deadline = None;
        self.retry_at = Some(now + Duration::from_secs(delay as u64));
    }
    pub fn seconds(&self, now: Instant) -> u64 {
        self.retry_at
            .map(|t| t.saturating_duration_since(now).as_millis().div_ceil(1000) as u64)
            .unwrap_or(0)
    }
    pub fn retry_due(&self, now: Instant) -> bool {
        self.retry_at.map(|t| now >= t).unwrap_or(false)
    }
    pub fn timed_out(&self, now: Instant) -> bool {
        self.deadline.map(|t| now >= t).unwrap_or(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logon_gate_retry_timeout_and_late_event() {
        let now = Instant::now();
        let mut machine = Machine::new();
        machine.connecting(now);
        assert_eq!(machine.state, AppState::Connecting);
        assert!(machine.timed_out(now + Duration::from_secs(46)));
        machine.failure(now, 10);
        assert_eq!(machine.state, AppState::Error);
        assert!(!machine.login_complete());
        assert_eq!(machine.seconds(now), 10);
        assert!(!machine.retry_due(now + Duration::from_secs(9)));
        assert!(machine.retry_due(now + Duration::from_secs(10)));
        machine.connecting(now);
        assert!(machine.login_complete());
        machine.failure(now, 10);
        assert_eq!(machine.state, AppState::Reconnecting);
    }
}
