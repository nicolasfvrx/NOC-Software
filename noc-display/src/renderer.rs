use crate::{
    assets::Assets,
    config::Config,
    scaling::{calculate_contain_rect, calculate_cover_rect, Rect},
    status::Status,
    text::{TextFrame, TextRenderer},
};
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        Graphics::{Direct2D::Common::*, Direct2D::*, Dxgi::Common::*},
    },
};

struct Surface {
    target: ID2D1HwndRenderTarget,
    background: Option<ID2D1Bitmap>,
    logo: Option<ID2D1Bitmap>,
    text_frame: Option<((u32, u32, Status, bool), TextFrame)>,
}

pub struct Renderer {
    factory: ID2D1Factory,
    surface: Option<Surface>,
    text: TextRenderer,
    animation_start: std::time::Instant,
}

impl Renderer {
    pub fn new(config: &Config, username: &str) -> Result<Self> {
        Ok(Self {
            factory: unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? },
            surface: None,
            text: TextRenderer::new(config, username)?,
            animation_start: std::time::Instant::now(),
        })
    }

    pub fn discard_surface(&mut self) {
        self.surface = None;
    }
    pub fn set_message(&mut self, message: &str) {
        if self.text.set_message(message) {
            if let Some(surface) = &mut self.surface {
                surface.text_frame = None;
            }
        }
    }

    pub fn paint(
        &mut self,
        hwnd: HWND,
        width: u32,
        height: u32,
        assets: &Assets,
        status: Status,
        preview: bool,
    ) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        unsafe {
            let size = D2D_SIZE_U { width, height };
            if self.surface.is_none() {
                // Direct2D 1.0 software rendering: no GPU/modern DirectX requirement.
                let target = self.factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_IGNORE,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        usage: D2D1_RENDER_TARGET_USAGE_NONE,
                        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    },
                )?;
                let load =
                    |image: &Option<windows::Win32::Graphics::Imaging::IWICFormatConverter>| {
                        image.as_ref().and_then(|image| {
                            match target.CreateBitmapFromWicBitmap(image, None) {
                                Ok(bitmap) => Some(bitmap),
                                Err(error) => {
                                    crate::log_error(error);
                                    None
                                }
                            }
                        })
                    };
                self.surface = Some(Surface {
                    text_frame: None,
                    background: load(&assets.background),
                    logo: load(&assets.logo),
                    target,
                });
            }
            let surface = self.surface.as_mut().unwrap();
            let current = surface.target.GetPixelSize();
            if current.width != width || current.height != height {
                if let Err(error) = surface.target.Resize(&size) {
                    self.discard_surface();
                    return Err(error);
                }
            }
            let viewport = (width as f32, height as f32);
            let logo_rect = surface.logo.as_ref().and_then(|bitmap| {
                let s = bitmap.GetPixelSize();
                let bounds = (viewport.0 * 0.4, viewport.1 * 0.4);
                calculate_contain_rect((s.width as f32, s.height as f32), bounds).map(|mut rect| {
                    let (dx, dy) = ((viewport.0 - bounds.0) / 2.0, (viewport.1 - bounds.1) / 2.0);
                    rect.left += dx;
                    rect.right += dx;
                    rect.top += dy;
                    rect.bottom += dy;
                    rect
                })
            });
            let text_key = (width, height, status, preview);
            if surface.text_frame.as_ref().map(|(key, _)| *key) != Some(text_key) {
                surface.text_frame = Some((
                    text_key,
                    self.text.prepare(
                        &surface.target,
                        viewport.0,
                        viewport.1,
                        logo_rect.map_or(viewport.1 * 0.5, |r| r.bottom),
                        status,
                        preview,
                    )?,
                ));
            }
            surface.target.BeginDraw();
            surface.target.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            if let Some(bitmap) = &surface.background {
                let s = bitmap.GetPixelSize();
                if let Some(rect) =
                    calculate_cover_rect((s.width as f32, s.height as f32), viewport)
                {
                    draw(&surface.target, bitmap, rect);
                }
            }
            if let (Some(bitmap), Some(rect)) = (&surface.logo, logo_rect) {
                draw(&surface.target, bitmap, rect);
            }
            surface.text_frame.as_ref().unwrap().1.draw(
                &surface.target,
                self.animation_start.elapsed().as_secs_f32(),
            );
            if let Err(error) = surface.target.EndDraw(None, None) {
                self.discard_surface();
                return Err(error);
            }
        }
        Ok(())
    }
}

unsafe fn draw(target: &ID2D1HwndRenderTarget, bitmap: &ID2D1Bitmap, rect: Rect) {
    target.DrawBitmap(
        bitmap,
        Some(&D2D_RECT_F {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }),
        1.0,
        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{System::Com::*, UI::WindowsAndMessaging::*};

    #[test]
    fn native_text_and_icons_render_across_states_and_resizes() -> Result<()> {
        // Hidden native window: exercises DirectWrite/Direct2D without taking focus.
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)?;
            let _com = Com;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("DisplayClient render test"),
                WS_POPUP,
                0,
                0,
                640,
                480,
                None,
                None,
                None,
                None,
            );
            if hwnd.0 == 0 {
                return Err(Error::from_win32());
            }
            let _window = Window(hwnd);
            let username = crate::identity::current_username()?;
            assert!(!username.is_empty());
            let config = Config {
                rdp: crate::config::RdpSettings {
                    username: "DOMAINE\\élodie".into(),
                    server: format!("rdp://serveur/{}", "chemin-très-long".repeat(12)),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut renderer = Renderer::new(&config, &username)?;
            let assets = Assets {
                background: None,
                logo: None,
            };
            for (w, h) in [(640, 480), (480, 640), (1920, 1080)] {
                for status in [
                    Status::Ready,
                    Status::Loading,
                    Status::ServerUnavailable,
                    Status::SessionEnded,
                ] {
                    renderer.paint(hwnd, w, h, &assets, status, true)?;
                    renderer.paint(hwnd, w, h, &assets, status, true)?; // Cached text frame.
                }
            }
            renderer.discard_surface();
            renderer.paint(hwnd, 640, 480, &assets, Status::Ready, false)?;
        }
        Ok(())
    }
}
