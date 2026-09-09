//! Fenetre de statut plein ecran (eframe / egui) + pont thread-safe
//! entre le worker asynchrone et le thread UI.
//!
//! Le thread UI ne fait QUE : afficher l'etat, mettre a jour les textes,
//! masquer / reafficher la fenetre. Aucune I/O bloquante ici.

use crate::config::Config;
use crate::logging;
use crate::state::KioskState;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct UiSnapshot {
    pub state: KioskState,
    pub primary: String,
    pub secondary: String,
    /// Troisieme ligne : compte a rebours, progression, detail technique.
    pub detail: String,
    pub visible: bool,
    /// Incremente a chaque appel de `show()` : force le thread UI a
    /// re-appliquer plein ecran + always-on-top + focus.
    pub show_epoch: u64,
}

pub struct UiBridge {
    inner: Mutex<UiSnapshot>,
    ctx: Mutex<Option<egui::Context>>,
    username: String,
}

pub type UiHandle = Arc<UiBridge>;

impl UiBridge {
    pub fn new(username: &str) -> Self {
        Self {
            inner: Mutex::new(UiSnapshot {
                state: KioskState::Starting,
                primary: KioskState::Starting.primary().to_string(),
                secondary: KioskState::Starting.secondary().to_string(),
                detail: String::new(),
                visible: true,
                show_epoch: 1,
            }),
            ctx: Mutex::new(None),
            username: username.to_string(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn attach(&self, ctx: egui::Context) {
        if let Ok(mut guard) = self.ctx.lock() {
            *guard = Some(ctx);
        }
    }

    pub fn snapshot(&self) -> UiSnapshot {
        self.inner
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    fn repaint(&self) {
        if let Ok(guard) = self.ctx.lock() {
            if let Some(ctx) = guard.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    /// Change d'etat avec les messages par defaut.
    pub fn set(&self, state: KioskState) {
        self.set_with(state, None);
    }

    /// Change d'etat en remplacant le message secondaire.
    pub fn set_with(&self, state: KioskState, secondary: Option<&str>) {
        let primary = state.primary().to_string();
        let secondary = secondary
            .map(|s| s.to_string())
            .unwrap_or_else(|| state.secondary().to_string())
            .replace("{username}", &self.username);

        let mut changed = false;
        if let Ok(mut guard) = self.inner.lock() {
            changed = guard.state != state || guard.secondary != secondary;
            guard.state = state;
            guard.primary = primary;
            guard.secondary = secondary.clone();
            guard.detail.clear();
        }
        if changed {
            logging::log(state.code(), &secondary);
        }
        self.repaint();
    }

    /// Met a jour la troisieme ligne (sans log) : compte a rebours, etc.
    pub fn set_detail(&self, detail: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.detail = detail.to_string();
        }
        self.repaint();
    }

    /// Affiche NOC Agent au premier plan (etats d'attente / erreur).
    /// Toujours accompagne d'une remise au premier plan, meme si la
    /// fenetre etait deja visible.
    pub fn show(&self) {
        let was_hidden = {
            match self.inner.lock() {
                Ok(mut guard) => {
                    let was = !guard.visible;
                    guard.visible = true;
                    guard.show_epoch = guard.show_epoch.wrapping_add(1);
                    was
                }
                Err(_) => false,
            }
        };
        if was_hidden {
            logging::log("UI", "écran de statut affiché");
        }
        self.repaint();
    }

    /// Masque NOC Agent : Firefox devient visible.
    /// NOC Agent reste lance en permanence.
    pub fn hide(&self) {
        let was_visible = {
            match self.inner.lock() {
                Ok(mut guard) => {
                    let was = guard.visible;
                    guard.visible = false;
                    was
                }
                Err(_) => false,
            }
        };
        if was_visible {
            logging::log("UI", "écran de statut masqué, Firefox au premier plan");
        }
        self.repaint();
    }
}

pub struct KioskApp {
    bridge: UiHandle,
    background: Option<egui::TextureHandle>,
    logo: Option<egui::TextureHandle>,
    last_visible: bool,
    last_show_epoch: u64,
    /// Tant que cet instant n'est pas passe, on reaffirme plein ecran +
    /// always-on-top a chaque frame : sur X11/XFCE le remapping d'une
    /// fenetre cachee n'est pas instantane et le panneau du bureau peut
    /// rester au-dessus.
    raise_until: Option<std::time::Instant>,
    username: String,
    assets_note: String,
}

impl KioskApp {
    pub fn new(cc: &eframe::CreationContext<'_>, bridge: UiHandle, cfg: &Config, base: &Path) -> Self {
        bridge.attach(cc.egui_ctx.clone());

        let bg_path = Config::asset_path(base, &cfg.ui.background);
        let logo_path = Config::asset_path(base, &cfg.ui.logo);

        let background = load_texture(&cc.egui_ctx, &bg_path, "background");
        let logo = load_texture(&cc.egui_ctx, &logo_path, "logo");

        let mut missing = Vec::new();
        if background.is_none() {
            missing.push(bg_path.display().to_string());
        }
        if logo.is_none() {
            missing.push(logo_path.display().to_string());
        }
        let assets_note = if missing.is_empty() {
            String::new()
        } else {
            logging::log("UI", &format!("assets manquants : {}", missing.join(", ")));
            format!("Assets manquants : {}", missing.join(", "))
        };

        Self {
            username: bridge.username().to_string(),
            bridge,
            background,
            logo,
            last_visible: true,
            last_show_epoch: 0,
            raise_until: None,
            assets_note,
        }
    }

    fn apply_visibility(&mut self, ctx: &egui::Context, snap: &UiSnapshot) {
        let visible = snap.visible;
        let re_show = visible && snap.show_epoch != self.last_show_epoch;
        self.last_show_epoch = snap.show_epoch;

        if visible == self.last_visible && !re_show {
            return;
        }
        self.last_visible = visible;
        if visible {
            // Fenetre remise devant : on reaffirme l'etat pendant ~2 s.
            self.raise_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.raise(ctx);
        } else {
            self.raise_until = None;
            // On relache d'abord l'always-on-top, sinon Firefox resterait
            // derriere une fenetre invisible sur certains gestionnaires.
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn raise(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for KioskApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.04, 0.05, 0.07, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snap = self.bridge.snapshot();
        self.apply_visibility(ctx, &snap);

        // Rafraichissement periodique : animation des points de progression
        // et mise a jour des comptes a rebours.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        // Fenetre tout juste reaffichee : on insiste plusieurs frames pour
        // passer devant Firefox et devant le panneau XFCE.
        if let Some(deadline) = self.raise_until {
            if std::time::Instant::now() < deadline {
                self.raise(ctx);
                ctx.request_repaint();
            } else {
                self.raise_until = None;
            }
        }

        if !snap.visible {
            return;
        }

        let frame = egui::Frame::none().fill(egui::Color32::from_rgb(10, 13, 18));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter();
            let time = ui.input(|i| i.time);

            draw_background(painter, rect, self.background.as_ref());

            // Voile sombre pour garantir la lisibilite du texte.
            painter.rect_filled(
                rect,
                0.0,
                egui::Color32::from_black_alpha(120),
            );

            let cx = rect.center().x;
            let h = rect.height();
            let w = rect.width();

            // --- Logo -----------------------------------------------------
            let text_top = match &self.logo {
                Some(logo) => {
                    let size = fit_inside(logo.size_vec2(), egui::vec2(w * 0.28, h * 0.24));
                    let logo_rect = egui::Rect::from_center_size(
                        egui::pos2(cx, rect.center().y - h * 0.12),
                        size,
                    );
                    painter.image(
                        logo.id(),
                        logo_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    logo_rect.bottom() + h * 0.08
                }
                None => rect.center().y - h * 0.02,
            };

            // --- Textes ---------------------------------------------------
            let primary_size = (h * 0.042).clamp(20.0, 56.0);
            let secondary_size = (h * 0.026).clamp(14.0, 32.0);
            let detail_size = (h * 0.022).clamp(12.0, 26.0);
            let wrap = w * 0.8;

            let mut y = text_top;
            y = draw_centered(
                painter,
                cx,
                y,
                &format!("{}{}", snap.primary, dots(time, &snap.primary)),
                primary_size,
                egui::Color32::from_rgb(245, 247, 250),
                wrap,
            ) + secondary_size * 0.9;

            if !snap.secondary.is_empty() {
                y = draw_centered(
                    painter,
                    cx,
                    y,
                    &snap.secondary,
                    secondary_size,
                    egui::Color32::from_rgb(198, 208, 220),
                    wrap,
                ) + detail_size * 0.8;
            }

            if !snap.detail.is_empty() {
                y = draw_centered(
                    painter,
                    cx,
                    y,
                    &snap.detail,
                    detail_size,
                    egui::Color32::from_rgb(150, 162, 178),
                    wrap,
                );
            }
            let _ = y;

            // --- Bandeau d'informations -----------------------------------
            let info_size = (h * 0.018).clamp(11.0, 20.0);
            let info_color = egui::Color32::from_rgb(120, 132, 148);
            painter.text(
                egui::pos2(rect.left() + 24.0, rect.bottom() - 24.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{}  •  {}", self.username, snap.state.code()),
                egui::FontId::monospace(info_size),
                info_color,
            );
            painter.text(
                egui::pos2(rect.right() - 24.0, rect.bottom() - 24.0),
                egui::Align2::RIGHT_BOTTOM,
                chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string(),
                egui::FontId::monospace(info_size),
                info_color,
            );
            painter.text(
                egui::pos2(rect.right() - 24.0, rect.top() + 24.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "NOC Agent v{} — Build {}",
                    env!("CARGO_PKG_VERSION"),
                    env!("NOC_AGENT_BUILD_TIME")
                ),
                egui::FontId::monospace(info_size),
                info_color,
            );
            if !self.assets_note.is_empty() {
                painter.text(
                    egui::pos2(rect.left() + 24.0, rect.top() + 24.0),
                    egui::Align2::LEFT_TOP,
                    &self.assets_note,
                    egui::FontId::monospace(info_size),
                    info_color,
                );
            }
        });
    }
}

/// Anime les points de suspension d'un message qui se termine par "…".
fn dots(time: f64, text: &str) -> &'static str {
    if !text.ends_with('…') {
        return "";
    }
    match ((time * 2.0) as i64) % 3 {
        0 => "",
        1 => " .",
        _ => " . .",
    }
}

/// Dessine le texte centre horizontalement, retourne le bas du bloc.
fn draw_centered(
    painter: &egui::Painter,
    center_x: f32,
    top: f32,
    text: &str,
    size: f32,
    color: egui::Color32,
    wrap_width: f32,
) -> f32 {
    let galley = painter.layout(
        text.to_string(),
        egui::FontId::proportional(size),
        color,
        wrap_width,
    );
    let pos = egui::pos2(center_x - galley.size().x / 2.0, top);
    let height = galley.size().y;
    painter.galley(pos, galley, color);
    top + height
}

/// Fond en mode "cover" : remplit l'ecran, ratio conserve, crop centre.
fn draw_background(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: Option<&egui::TextureHandle>,
) {
    let Some(texture) = texture else {
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(10, 13, 18));
        return;
    };

    let img = texture.size_vec2();
    if img.x <= 0.0 || img.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / img.x).max(rect.height() / img.y);
    let drawn = img * scale;
    let u = (rect.width() / drawn.x).clamp(0.0, 1.0);
    let v = (rect.height() / drawn.y).clamp(0.0, 1.0);
    let uv = egui::Rect::from_min_max(
        egui::pos2((1.0 - u) / 2.0, (1.0 - v) / 2.0),
        egui::pos2((1.0 + u) / 2.0, (1.0 + v) / 2.0),
    );
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}

/// Redimensionne en conservant le ratio, sans jamais agrandir au-dela
/// de la taille native.
fn fit_inside(image: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    if image.x <= 0.0 || image.y <= 0.0 {
        return max;
    }
    let scale = (max.x / image.x).min(max.y / image.y).min(1.0);
    image * scale
}

fn load_texture(
    ctx: &egui::Context,
    path: &PathBuf,
    name: &str,
) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
}
