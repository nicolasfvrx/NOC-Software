//! DirectWrite 1.0 text shaping, wrapping and measurement in physical pixels.
use crate::{config::Config, status::Status};
use windows::{
    core::*,
    Win32::Graphics::{Direct2D::Common::*, Direct2D::*, DirectWrite::*},
    Win32::System::SystemInformation::GetLocalTime,
};

pub struct TextRenderer {
    factory: IDWriteFactory,
    details: Vec<u16>,
    username: String,
    /// Code court de l'etat courant (AppState::code()), affiche en bas a
    /// gauche a cote du nom d'utilisateur ; mis a jour en dehors du cycle
    /// prepare()/draw() habituel, voir `set_state_code`.
    state_code: &'static str,
    message_override: Option<String>,
}

pub struct TextFrame {
    build: IDWriteTextLayout,
    build_origin: D2D_POINT_2F,
    message: IDWriteTextLayout,
    details: IDWriteTextLayout,
    foreground: ID2D1SolidColorBrush,
    secondary: ID2D1SolidColorBrush,
    shadow: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    status: Status,
    icon_center: D2D_POINT_2F,
    icon_radius: f32,
    text_bounds: D2D_RECT_F,
    message_origin: D2D_POINT_2F,
    details_origin: D2D_POINT_2F,
}

impl TextRenderer {
    pub fn new(config: &Config, username: &str) -> Result<Self> {
        Ok(Self {
            factory: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? },
            details: config.details(username).encode_utf16().collect(),
            username: username.to_owned(),
            state_code: "STARTING",
            message_override: None,
        })
    }
    pub fn set_message(&mut self, text: &str) -> bool {
        if self.message_override.as_deref() == Some(text) {
            return false;
        }
        self.message_override = Some(text.to_owned());
        true
    }
    pub fn set_state_code(&mut self, code: &'static str) {
        self.state_code = code;
    }

    unsafe fn layout(
        &self,
        text: &[u16],
        size: f32,
        width: f32,
        height: f32,
    ) -> Result<(IDWriteTextLayout, f32)> {
        let format = self.factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("fr-FR"),
        )?;
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
        let layout = self
            .factory
            .CreateTextLayout(text, &format, width, height)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        Ok((layout, if text.is_empty() { 0.0 } else { metrics.height }))
    }

    /// Measure before BeginDraw, so a COM failure cannot leave a frame open.
    pub fn prepare(
        &self,
        target: &ID2D1HwndRenderTarget,
        width: f32,
        height: f32,
        logo_bottom: f32,
        status: Status,
        preview: bool,
    ) -> Result<TextFrame> {
        unsafe {
            let padding = height * 0.012;
            let top = logo_bottom + height * 0.03;
            let icon_radius = (height * 0.011).min(13.0);
            let icon_space = if status == Status::Ready {
                0.0
            } else {
                icon_radius * 2.0 + padding
            };
            let available = (height * 0.97 - top - padding * 2.0 - icon_space).max(0.1);
            let text_width = (width * 0.86).max(0.1);
            let gap = height * 0.018;
            let message_text = if preview {
                format!("Aperçu — {}", status.message())
            } else {
                self.message_override
                    .clone()
                    .unwrap_or_else(|| status.message().to_owned())
            };
            let message_text: Vec<u16> = message_text.encode_utf16().collect();
            let mut size = (height * 0.027).min(30.0);
            let (message, mh, details, dh) = loop {
                let (message, mh) = self.layout(&message_text, size, text_width, available)?;
                let (details, dh) =
                    self.layout(&self.details, size * 0.62, text_width, available)?;
                if mh + gap + dh <= available || size <= 0.1 {
                    break (message, mh, details, dh);
                }
                size *= 0.85;
            };
            let margin = (height * 0.018).clamp(8.0, 24.0).min(width * 0.05);
            let build_text: Vec<u16> = concat!(
                "NOC Display v",
                env!("CARGO_PKG_VERSION"),
                " — Build ",
                env!("DISPLAYCLIENT_BUILD_TIME")
            )
            .encode_utf16()
            .collect();
            let (build, _) = self.layout(
                &build_text,
                (height * 0.015).clamp(10.0, 16.0),
                (width - margin * 2.0).max(0.1),
                40.0,
            )?;
            build.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            build.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(TextFrame {
                build,
                build_origin: D2D_POINT_2F {
                    x: margin,
                    y: margin,
                },
                status,
                icon_center: D2D_POINT_2F {
                    x: width * 0.5,
                    y: top + padding + icon_radius,
                },
                icon_radius,
                accent: target.CreateSolidColorBrush(
                    &if status.is_error() {
                        D2D1_COLOR_F {
                            r: 1.0,
                            g: 0.4,
                            b: 0.35,
                            a: 1.0,
                        }
                    } else {
                        D2D1_COLOR_F {
                            r: 0.4,
                            g: 0.75,
                            b: 1.0,
                            a: 1.0,
                        }
                    },
                    None,
                )?,
                message,
                details,
                foreground: target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    None,
                )?,
                secondary: target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.70,
                        g: 0.79,
                        b: 0.89,
                        a: 1.0,
                    },
                    None,
                )?,
                shadow: target.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.18,
                    },
                    None,
                )?,
                // Invisible clipping bounds only; never draw a panel behind the text.
                text_bounds: D2D_RECT_F {
                    left: width * 0.05,
                    top,
                    right: width * 0.95,
                    bottom: (top + padding * 2.0 + icon_space + mh + gap + dh).min(height * 0.97),
                },
                message_origin: D2D_POINT_2F {
                    x: width * 0.07,
                    y: top + padding + icon_space,
                },
                details_origin: D2D_POINT_2F {
                    x: width * 0.07,
                    y: top + padding + icon_space + mh + gap,
                },
            })
        }
    }

    /// Bandeau bas : `utilisateur • etat` a gauche, heure locale en direct a
    /// droite. Construit a chaque appel plutot que mis en cache dans
    /// `TextFrame` : contrairement au reste du texte, l'heure change sans
    /// qu'aucune des cles de cache (taille, statut, apercu) ne bouge.
    /// Appele par `Renderer::paint()` a chaque repaint (voir le timer
    /// applicatif de 250 ms dans window.rs), independamment du throttling
    /// habituel sur les autres textes.
    pub unsafe fn draw_live_info(
        &self,
        target: &ID2D1HwndRenderTarget,
        width: f32,
        height: f32,
    ) -> Result<()> {
        let margin = (height * 0.018).clamp(8.0, 24.0).min(width * 0.05);
        let size = (height * 0.015).clamp(10.0, 16.0);
        let box_width = (width - margin * 2.0).max(0.1);

        let identity_text: Vec<u16> = format!("{}  •  {}", self.username, self.state_code)
            .encode_utf16()
            .collect();
        let (identity, identity_h) = self.layout(&identity_text, size, box_width, 40.0)?;
        identity.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        identity.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let clock_text: Vec<u16> = local_time_string().encode_utf16().collect();
        let (clock, clock_h) = self.layout(&clock_text, size, box_width, 40.0)?;
        clock.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        clock.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let secondary = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.70,
                g: 0.79,
                b: 0.89,
                a: 1.0,
            },
            None,
        )?;
        let shadow = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.18,
            },
            None,
        )?;

        let y = height - margin - identity_h.max(clock_h);
        for layout in [&identity, &clock] {
            let origin = D2D_POINT_2F { x: margin, y };
            target.DrawTextLayout(
                D2D_POINT_2F {
                    x: origin.x + 1.0,
                    y: origin.y + 1.0,
                },
                layout,
                &shadow,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
            target.DrawTextLayout(origin, layout, &secondary, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
        Ok(())
    }
}

fn local_time_string() -> String {
    let time = unsafe { GetLocalTime() };
    format!(
        "{:02}/{:02}/{} {:02}:{:02}:{:02}",
        time.wDay, time.wMonth, time.wYear, time.wHour, time.wMinute, time.wSecond
    )
}

impl TextFrame {
    pub unsafe fn draw(&self, target: &ID2D1HwndRenderTarget, seconds: f32) {
        target.PushAxisAlignedClip(&self.text_bounds, D2D1_ANTIALIAS_MODE_ALIASED);
        if self.status != Status::Ready {
            self.draw_icon(target, seconds);
        }
        // A subtle shadow follows the glyphs; the original image remains visible
        // everywhere around the lettering, without a rectangle or a banner.
        for (dx, dy) in [(-1.0, 1.0), (1.0, 1.0), (0.0, 2.0)] {
            for (origin, layout) in [
                (self.message_origin, &self.message),
                (self.details_origin, &self.details),
            ] {
                target.DrawTextLayout(
                    D2D_POINT_2F {
                        x: origin.x + dx,
                        y: origin.y + dy,
                    },
                    layout,
                    &self.shadow,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                );
            }
        }
        target.DrawTextLayout(
            self.message_origin,
            &self.message,
            &self.foreground,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
        );
        target.DrawTextLayout(
            self.details_origin,
            &self.details,
            &self.secondary,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
        );
        target.PopAxisAlignedClip();
        target.DrawTextLayout(
            D2D_POINT_2F {
                x: self.build_origin.x,
                y: self.build_origin.y + 1.0,
            },
            &self.build,
            &self.shadow,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
        );
        target.DrawTextLayout(
            self.build_origin,
            &self.build,
            &self.secondary,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
        );
    }

    unsafe fn draw_icon(&self, target: &ID2D1HwndRenderTarget, seconds: f32) {
        let center = self.icon_center;
        let radius = self.icon_radius;
        if self.status.is_loading() {
            let phase = (seconds * 10.0) as usize % 12;
            for index in 0..12 {
                let angle =
                    index as f32 * std::f32::consts::TAU / 12.0 - std::f32::consts::FRAC_PI_2;
                let age = (phase + 12 - index) % 12;
                self.accent.SetOpacity(1.0 - age as f32 / 15.0);
                target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: center.x + angle.cos() * radius * 0.75,
                            y: center.y + angle.sin() * radius * 0.75,
                        },
                        radiusX: radius * 0.16,
                        radiusY: radius * 0.16,
                    },
                    &self.accent,
                );
            }
            self.accent.SetOpacity(1.0);
        } else {
            target.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: center,
                    radiusX: radius,
                    radiusY: radius,
                },
                &self.accent,
                (radius * 0.12).max(0.5),
                None,
            );
            let direction = if self.status.is_error() { -1.0 } else { 1.0 };
            target.DrawLine(
                D2D_POINT_2F {
                    x: center.x,
                    y: center.y - radius * 0.35 * direction,
                },
                D2D_POINT_2F {
                    x: center.x,
                    y: center.y + radius * 0.25 * direction,
                },
                &self.accent,
                (radius * 0.16).max(0.5),
                None,
            );
            target.FillEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F {
                        x: center.x,
                        y: center.y - radius * 0.55 * direction,
                    },
                    radiusX: radius * 0.1,
                    radiusY: radius * 0.1,
                },
                &self.accent,
            );
        }
    }
}
