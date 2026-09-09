#![windows_subsystem = "windows"]
#![allow(non_snake_case)] // Preserve the requested executable name: noc-display.exe.

mod app;
mod assets;
mod config;
mod credentials;
mod identity;
mod logging;
mod rdp;
mod renderer;
mod scaling;
mod state;
mod status;
mod text;
mod window;

use logging::write as log_error;
use std::path::PathBuf;
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

fn executable_dir() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(|p| p.to_owned()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Executable directory unavailable",
        )
    })
}

fn main() {
    // OLE STA lifetime outlives every ActiveX, WIC and Direct2D object.
    unsafe {
        if std::env::args().any(|arg| arg == "--set-credentials") {
            if let Err(error) = credentials::provision() {
                eprintln!("Échec : {error}");
                log_error(format!(
                    "Credential provisioning failed HRESULT={:08x}",
                    error.code().0
                ));
                std::process::exit(1);
            }
            return;
        }
        if let Err(error) = OleInitialize(None) {
            log_error(error);
            return;
        }
        if let Err(error) = app::run() {
            log_error(error);
        }
        OleUninitialize();
    }
}
