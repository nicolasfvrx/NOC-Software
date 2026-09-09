//! RDP backend contract. The window remains the owner of the fullscreen surface.
use windows::{core::Result, Win32::UI::WindowsAndMessaging::MSG};
pub mod active_x;
pub mod dialog_guard;
pub mod dispatch;
pub mod events;
pub mod host;
pub mod interfaces;
pub mod settings;

pub trait RdpClient {
    fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn resize(&self, width: i32, height: i32) -> Result<()>;
    fn set_visible(&self, visible: bool);
    fn translate_accelerator(&self, message: &MSG) -> bool;
}
