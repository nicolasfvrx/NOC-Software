//! Orchestration on the UI STA, outside window procedures and COM callbacks.
use crate::{
    config::Config,
    credentials::{ConfiguredProvider, CredentialProvider},
    rdp::{
        active_x::ActiveX,
        events::{EventKind, Queue},
        RdpClient,
    },
    state::{AppState, Machine},
    status::Status,
    window::Window,
};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::{core::*, Win32::UI::WindowsAndMessaging::*};

/// Delay before the first RDP connection attempt, so the branded startup
/// frame is visible for a moment instead of connecting instantly.
const STARTUP_DELAY: Duration = Duration::from_secs(8);
const MANAGER_UNREACHABLE_AFTER: Duration = Duration::from_secs(60);

/// Current RDP connection state, published for the heartbeat thread
/// (health.rs) to read. A plain global is simplest here: this process hosts
/// exactly one App, and the heartbeat thread has no other way to reach it
/// across the Win32 message loop.
static CURRENT_STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

pub fn current_status() -> AppState {
    *CURRENT_STATE
        .get_or_init(|| Mutex::new(AppState::Starting))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn publish_status(state: AppState) {
    if let Ok(mut guard) = CURRENT_STATE
        .get_or_init(|| Mutex::new(AppState::Starting))
        .lock()
    {
        *guard = state;
    }
}

pub struct App {
    window: Window,
    client: Option<ActiveX>,
    queue: Queue,
    generation: u64,
    config: Config,
    machine: Machine,
    username: String,
    resolution: (i32, i32),
    last_message: String,
    transport_connected: bool,
    ready_timer: Option<usize>,
    /// Aucun config.toml trouve (ni par compte, ni global) : `refresh()`
    /// affiche un message dedie plutot que le texte d'echec RDP generique.
    config_missing: bool,
    /// config.toml present et `provides_rdp` actif, mais Manager ne connait
    /// pas ce kiosque (404) : message distinct de `config_missing`, puisque
    /// le probleme n'est pas local mais cote Manager.
    kiosk_missing: bool,
    manager_unreachable_since: Option<Instant>,
}
impl App {
    fn new() -> Result<Self> {
        let window = Window::create()?;
        let username = crate::identity::current_username().unwrap_or_else(|e| {
            crate::log_error(format!("GetUserNameW HRESULT={:08x}", e.code().0));
            "Indisponible".into()
        });
        let config = Config::default();
        window.configure(&config, &username);
        window.update(Status::Loading, "Initialisation…", true);
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(window.hwnd);
        }
        let resolution = window.size();
        Ok(Self {
            window,
            client: None,
            queue: Default::default(),
            generation: 0,
            config,
            machine: Machine::new(),
            username,
            resolution,
            last_message: String::new(),
            transport_connected: false,
            ready_timer: None,
            config_missing: false,
            kiosk_missing: false,
            manager_unreachable_since: None,
        })
    }
    fn schedule_first_attempt(&mut self) {
        self.machine.retry_at = Some(Instant::now() + STARTUP_DELAY);
        crate::log_error(format!(
            "RDP first attempt delayed {} seconds",
            STARTUP_DELAY.as_secs()
        ));
    }
    fn attempt(&mut self) {
        self.cancel_ready_timer();
        self.transport_connected = false;
        self.generation = self.generation.wrapping_add(1);
        self.client.take();
        match Config::read() {
            Ok(config) => {
                self.config_missing = false;
                self.kiosk_missing = false;
                self.config = config;
                self.window.configure(&self.config, &self.username);
            }
            Err(message) => {
                // Pas de fichier du tout (ni par compte, ni global) : message
                // dedie plutot que le texte d'echec RDP generique, qui ferait
                // croire a une boucle de reconnexion RDP normale.
                self.config_missing = !Config::exists();
                self.kiosk_missing = false;
                self.failed(&message);
                return;
            }
        }
        if !self.config.rdp.enabled {
            self.manager_unreachable_since = None;
            self.machine = Machine::new();
            self.window.set_state_code(self.machine.state.code());
            self.window
                .update(Status::Ready, "Bienvenue. Votre poste est prêt.", false);
            self.last_message.clear();
            return;
        }
        // Show the connection screen before the synchronous Manager request.
        self.machine.connecting(Instant::now());
        if self.config.manager.provides_rdp {
            self.manager_unreachable_since.get_or_insert(Instant::now());
        }
        self.last_message.clear();
        self.refresh();
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(self.window.hwnd);
        }
        if self.config.manager.provides_rdp {
            match crate::health::fetch_rdp_config(
                &self.config.manager.host,
                self.config.manager.port,
                &self.username,
            ) {
                // Manager has no kiosk record for this Windows account name :
                // distinct from a network/server failure, someone needs to go
                // create/configure this kiosk on Manager.
                Ok(crate::health::RdpConfigFetch::KioskNotFound) => {
                    self.manager_unreachable_since = None;
                    self.kiosk_missing = true;
                    self.failed("RDP config: kiosk not found on Manager");
                    return;
                }
                // Manager turned this kiosk's RDP off: same idle screen as
                // the local !rdp.enabled case above, not a connection error.
                Ok(crate::health::RdpConfigFetch::Found(remote)) if !remote.enabled => {
                    self.manager_unreachable_since = None;
                    self.kiosk_missing = false;
                    self.machine = Machine::new();
                    self.window.set_state_code(self.machine.state.code());
                    self.window
                        .update(Status::Ready, "Bienvenue. Votre poste est prêt.", false);
                    self.last_message.clear();
                    return;
                }
                Ok(crate::health::RdpConfigFetch::Found(remote))
                    if remote.server.trim().is_empty() =>
                {
                    self.manager_unreachable_since = None;
                    self.kiosk_missing = false;
                    self.failed("RDP config from Manager: enabled but no server configured");
                    return;
                }
                Ok(crate::health::RdpConfigFetch::Found(remote)) => {
                    self.manager_unreachable_since = None;
                    self.kiosk_missing = false;
                    // Effective settings for this attempt only: Config::read()
                    // overwrites self.config wholesale at the top of the next
                    // attempt(), so nothing here leaks across reconnects.
                    self.config.rdp.server = remote.server;
                    self.config.rdp.port = remote.port;
                    self.config.rdp.username = if remote.username.trim().is_empty() {
                        self.username.clone()
                    } else {
                        remote.username
                    };
                    self.config.rdp.ignore_certificate_errors = remote.ignore_certificate_errors;
                    // None keeps ConfiguredProvider's existing DPAPI fallback.
                    self.config.rdp.password = remote.password;
                }
                Err(e) => {
                    if self.manager_unreachable_since.is_none() {
                        self.manager_unreachable_since = Some(Instant::now());
                    }
                    self.failed(&format!(
                        "RDP config fetch from Manager failed HRESULT={:08x}",
                        e.code().0
                    ));
                    return;
                }
            }
        } else {
            self.manager_unreachable_since = None;
        }
        self.machine.connecting(Instant::now());
        self.last_message.clear();
        self.refresh();
        // Ensure the branded connecting frame is presented before any ActiveX calls.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(self.window.hwnd);
        }
        let result = (|| -> Result<ActiveX> {
            let password = ConfiguredProvider.load_credentials(&self.config.rdp).map_err(|e| {
                crate::log_error(format!("RDP credentials unavailable; configure password or provision DPAPI for this account. HRESULT={:08x}", e.code().0));
                e
            })?;
            self.resolution = self.window.size();
            let client = ActiveX::create(
                self.window.hwnd,
                self.resolution.0,
                self.resolution.1,
                self.generation,
                self.queue.clone(),
            )?;
            crate::rdp::settings::configure(
                &client.dispatch,
                &self.config.rdp,
                self.resolution.0,
                self.resolution.1,
                self.window.hwnd,
            )?;
            client.set_password(&password.0)?;
            crate::rdp::settings::finalize_xrdp_credentials(&client.dispatch)?;
            crate::log_error(format!(
                "RDP connecting to {}:{}",
                self.config.rdp.server, self.config.rdp.port
            ));
            client.connect()?;
            Ok(client)
        })();
        match result {
            Ok(client) => self.client = Some(client),
            Err(e) => self.failed(&format!(
                "RDP preparation/Connect failed HRESULT={:08x}",
                e.code().0
            )),
        }
    }
    fn failed(&mut self, reason: &str) {
        self.cancel_ready_timer();
        self.transport_connected = false;
        if let Some(client) = &self.client {
            client.signals.hide();
        }
        self.window.focus();
        self.machine
            .failure(Instant::now(), self.config.rdp.retry_seconds);
        self.last_message.clear();
        self.refresh();
        crate::log_error(reason);
        crate::log_error(format!(
            "retry in {} seconds",
            self.config.rdp.retry_seconds
        ));
        self.client.take();
    }
    fn cancel_ready_timer(&mut self) {
        if let Some(id) = self.ready_timer.take() {
            unsafe {
                let _ = KillTimer(self.window.hwnd, id);
            }
        }
    }
    fn mark_remote_session_ready(&mut self, source: &str) {
        if self.machine.state != AppState::Connecting {
            return;
        }
        let Some(client) = self.client.as_ref() else {
            return;
        };
        if client.signals.failed.get() {
            return;
        }
        let (width, height) = self.window.size();
        if client.resize(width, height).is_err() {
            self.failed("RDP ready resize failed");
            return;
        }
        if client.signals.failed.get() {
            return;
        }
        self.cancel_ready_timer();
        if self.machine.login_complete() {
            self.window.update(
                Status::SessionEnded,
                &self.config.ui.disconnected_text,
                false,
            );
            // The full-size opaque child covers the branded frame, ready behind it on failure.
            self.client.as_ref().unwrap().set_visible(true);
            crate::log_error(format!("RDP session ready source={source}"));
        }
    }
    fn events(&mut self) {
        loop {
            // Never hold the queue borrow across a COM call or reentrant paint.
            let event = self.queue.borrow_mut().pop_front();
            let Some(event) = event else {
                break;
            };
            if event.generation != self.generation || self.client.is_none() {
                continue;
            }
            match event.kind {
                EventKind::Connecting => crate::log_error("RDP OnConnecting"),
                EventKind::TransportConnected => {
                    if self.machine.state == AppState::Connecting
                        && !self.transport_connected
                        && !self.client.as_ref().unwrap().signals.failed.get()
                    {
                        self.transport_connected = true;
                        self.client.as_ref().unwrap().set_visible(false);
                        let id = crate::rdp::events::ready_timer_id(self.generation);
                        if unsafe { SetTimer(self.window.hwnd, id, 2000, None) } != 0 {
                            self.ready_timer = Some(id);
                        } else {
                            self.failed("RDP ready timer creation failed");
                        }
                        crate::log_error("RDP OnConnected (hidden; ready timer 2000 ms)");
                    }
                }
                EventKind::LoginComplete => {
                    self.mark_remote_session_ready("OnLoginComplete");
                }
                EventKind::RemoteDesktopSizeChanged => {
                    if self.transport_connected {
                        self.mark_remote_session_ready("OnRemoteDesktopSizeChange");
                    }
                }
                EventKind::Disconnected(code) => {
                    let extended = match self.client.as_ref().unwrap().extended_disconnect_reason()
                    {
                        Ok(value) => {
                            let description = crate::rdp::dispatch::error_description(
                                &self.client.as_ref().unwrap().dispatch,
                                code,
                                value,
                            )
                            .unwrap_or_else(|error| {
                                format!("unavailable HRESULT=0x{:08x}", error.code().0 as u32)
                            });
                            format!(
                                "{value} hex=0x{:08x} GetErrorDescription={description:?}",
                                value as u32
                            )
                        }
                        Err(error) => {
                            format!("unavailable HRESULT=0x{:08x}", error.code().0 as u32)
                        }
                    };
                    self.failed(&format!(
                        "RDP disconnected reason={code} extended={extended}"
                    ))
                }
                EventKind::Fatal(code) => self.failed(&format!("RDP fatal reason={code}")),
                EventKind::Warning(code) => self.failed(&format!("RDP warning reason={code}")),
                EventKind::InteractionRequired { source, code } => self.failed(&format!(
                    "RDP interaction refused source={source} code={code} hex=0x{:08x}",
                    code as u32
                )),
                EventKind::AutoReconnecting => {
                    self.failed("RDP built-in reconnect cancelled; application retry policy")
                }
                EventKind::AutoReconnected => {
                    // Never bypass the logon visibility gate with this optional event.
                    self.failed(
                        "RDP unexpected OnAutoReconnected; new controlled attempt required",
                    );
                }
            }
        }
    }
    fn tick(&mut self) {
        self.events();
        if self.window.context.resized.replace(false) {
            let size = self.window.size();
            if size != self.resolution {
                self.resolution = size;
                if self.client.is_some() {
                    self.failed("RDP display size changed; reconnect with new desktop dimensions");
                }
            }
        }
        let now = Instant::now();
        if self.machine.timed_out(now) {
            self.failed("RDP logon timeout (45 seconds)");
        }
        if self.machine.retry_due(now) {
            self.attempt();
        }
        self.refresh();
    }
    fn refresh(&mut self) {
        publish_status(self.machine.state);
        self.window.set_state_code(self.machine.state.code());
        let (status, text, animate) = match self.machine.state {
            AppState::Starting | AppState::Connected => return,
            AppState::Connecting => (
                Status::Loading,
                if self.config.manager.provides_rdp
                    && self.manager_unreachable_since.is_some()
                {
                    self.config.ui.manager_connecting_text.clone()
                } else {
                    self.config.ui.connecting_text.clone()
                },
                true,
            ),
            // Pas d'erreur RDP a proprement parler : rien a corriger sur cette
            // machine que recharger un fichier. Pas de compte a rebours (ca
            // ferait croire a une vraie boucle d'echec RDP) : juste l'icone
            // animee, pour montrer que ca continue de verifier tout seul.
            AppState::Error | AppState::Reconnecting
                if self.config_missing || self.kiosk_missing =>
            {
                let title = if self.config_missing {
                    "Aucune configuration trouvée…"
                } else {
                    "Aucune configuration kiosk trouvée…"
                };
                (Status::Loading, title.to_string(), true)
            }
            AppState::Error | AppState::Reconnecting
                if self.manager_unreachable_since.is_some_and(|since| {
                    since.elapsed() < MANAGER_UNREACHABLE_AFTER
                }) =>
            {
                (
                    Status::Loading,
                    self.config.ui.manager_connecting_text.clone(),
                    true,
                )
            }
            AppState::Error | AppState::Reconnecting
                if self.manager_unreachable_since.is_some() =>
            {
                let countdown = self.config.ui.reconnecting_text.replace(
                    "{seconds}",
                    &self.machine.seconds(Instant::now()).to_string(),
                );
                (
                    Status::ServerUnavailable,
                    format!("{}\n{}", self.config.ui.manager_unreachable_text, countdown),
                    false,
                )
            }
            AppState::Error | AppState::Reconnecting => {
                let title = if self.machine.state == AppState::Error {
                    self.config.ui.error_text.clone()
                } else {
                    self.config.ui.disconnected_text.clone()
                };
                let countdown = self.config.ui.reconnecting_text.replace(
                    "{seconds}",
                    &self.machine.seconds(Instant::now()).to_string(),
                );
                (
                    Status::ServerUnavailable,
                    format!("{title}\n{countdown}"),
                    false,
                )
            }
        };
        if text != self.last_message {
            self.window.update(status, &text, animate);
            self.last_message = text;
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.cancel_ready_timer();
        self.client.take();
        crate::log_error("STOP application");
    }
}
pub fn run() -> Result<()> {
    crate::log_error(concat!(
        "START application build=",
        env!("DISPLAYCLIENT_BUILD_TIME")
    ));
    let mut app = App::new()?;
    crate::health::spawn(app.username.clone());
    app.schedule_first_attempt();
    unsafe {
        let mut message = MSG::default();
        while !app.window.context.quit.get() {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                return Err(Error::from_win32());
            }
            if result == 0 || app.window.exit_shortcut(&message) {
                break;
            }
            if message.hwnd == app.window.hwnd
                && message.message == WM_TIMER
                && app.ready_timer == Some(message.wParam.0)
            {
                // Drain COM failures first; stale timers cannot expose a disconnected child.
                app.events();
                if app.ready_timer == Some(message.wParam.0) {
                    app.cancel_ready_timer();
                    if app
                        .client
                        .as_ref()
                        .map(|c| !c.signals.failed.get() && c.is_connected().unwrap_or(false))
                        .unwrap_or(false)
                    {
                        app.mark_remote_session_ready("timer 2000 ms");
                    }
                }
                app.tick();
                continue;
            }
            let consumed = app
                .client
                .as_ref()
                .filter(|_| app.machine.state == AppState::Connected)
                .map(|c| c.translate_accelerator(&message))
                .unwrap_or(false);
            if !consumed {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            app.tick();
        }
    }
    Ok(())
}
