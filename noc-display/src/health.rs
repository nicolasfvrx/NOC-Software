//! Deux usages de NOC Manager, tous deux optionnels et desactives par
//! defaut (Display n'a historiquement aucune dependance a Manager, voir
//! README) :
//! - heartbeat periodique (statut/version/build), consomme par `/metrics`
//!   (Prometheus / Grafana) ;
//! - recuperation de la configuration RDP centralisee (`manager.provides_rdp`),
//!   utilisee par `app.rs` a chaque tentative de connexion.
//!
//! Implemente avec WinHTTP natif plutot qu'une bibliotheque HTTP tierce,
//! pour rester coherent avec le reste de ce binaire (aucune dependance
//! au-dela de la crate `windows`, hormis `serde_json` pour le sens
//! Manager -> Display, voir Cargo.toml) et garder l'executable leger.

use crate::config::Config;
use serde::Deserialize;
use std::ffi::c_void;
use std::time::Duration;
use windows::{core::*, Win32::Foundation::E_FAIL, Win32::Networking::WinHttp::*};

/// Lance le thread de heartbeat. Ne bloque jamais l'affichage : toute
/// erreur reseau est journalisee puis ignoree jusqu'au prochain envoi.
pub fn spawn(username: String) {
    let _ = std::thread::Builder::new()
        .name("noc-display-heartbeat".to_string())
        .spawn(move || run(username));
}

fn run(username: String) {
    loop {
        let config = Config::read().unwrap_or_default();
        if !config.manager.enabled || config.manager.host.trim().is_empty() {
            std::thread::sleep(Duration::from_secs(30));
            continue;
        }
        let interval = Duration::from_secs(config.manager.heartbeat_seconds.max(1) as u64);
        let path = format!("/api/heartbeat/display/{username}");
        let body = format!(
            r#"{{"version":"{}","build":"{}","state":"{}"}}"#,
            env!("CARGO_PKG_VERSION"),
            env!("DISPLAYCLIENT_BUILD_TIME"),
            crate::app::current_status().code()
        );
        if let Err(error) = post(&config.manager.host, config.manager.port, &path, &body) {
            crate::log_error(format!("Heartbeat HRESULT={:08x}", error.code().0));
        }
        std::thread::sleep(interval);
    }
}

/// Configuration RDP centralisee, telle que servie par
/// `GET /api/kiosk/{username}/rdp`.
#[derive(Debug, Deserialize)]
pub struct RdpConfigResponse {
    pub enabled: bool,
    #[serde(default)]
    pub server: String,
    #[serde(default = "default_rdp_port")]
    pub port: u16,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub ignore_certificate_errors: bool,
}
fn default_rdp_port() -> u16 {
    3389
}

/// Resultat de la recuperation de la config RDP : distingue "Manager ne
/// connait pas ce kiosque" (404, cas normal si personne ne l'a encore
/// configure cote Manager) d'un vrai echec reseau/serveur, pour que
/// `app.rs` puisse afficher un message adapte a chacun.
pub enum RdpConfigFetch {
    Found(RdpConfigResponse),
    KioskNotFound,
}

/// Recupere la configuration RDP du kiosque `username` (le compte Windows
/// courant) depuis NOC Manager. Appele de maniere synchrone par
/// `app.rs::attempt()` : un echec doit interrompre la tentative de
/// connexion en cours, exactement comme une erreur RDP locale.
pub fn fetch_rdp_config(host: &str, port: u16, username: &str) -> Result<RdpConfigFetch> {
    let path = format!("/api/kiosk/{username}/rdp");
    match get(host, port, &path)? {
        HttpGet::NotFound => Ok(RdpConfigFetch::KioskNotFound),
        HttpGet::Body(body) => serde_json::from_slice(&body)
            .map(RdpConfigFetch::Found)
            .map_err(|e| Error::new(E_FAIL, format!("réponse JSON invalide : {e}").into())),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Owns one WinHTTP handle (session/connect/request), closed on drop.
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn checked(handle: *mut c_void) -> Result<Handle> {
    if handle.is_null() {
        Err(Error::from_win32())
    } else {
        Ok(Handle(handle))
    }
}

/// Session + connexion + requete WinHTTP, prêtes pour `WinHttpSendRequest`.
/// Les trois handles doivent rester en vie ensemble : les garder dans un
/// seul struct evite de s'emmeler dans l'ordre de fermeture (RAII suffit).
struct Connection {
    _session: Handle,
    _connect: Handle,
    request: Handle,
}

fn open(host: &str, port: u16, verb: &str, path: &str) -> Result<Connection> {
    unsafe {
        let agent = wide(concat!("noc-display/", env!("CARGO_PKG_VERSION")));
        let session = checked(WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000)?;

        let host_w = wide(host);
        let connect = checked(WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0))?;

        let verb_w = wide(verb);
        let path_w = wide(path);
        let request = checked(WinHttpOpenRequest(
            connect.0,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_NULL_CODEPAGE,
        ))?;

        Ok(Connection {
            _session: session,
            _connect: connect,
            request,
        })
    }
}

unsafe fn status_code(request: *mut c_void) -> Result<u32> {
    let mut status: u32 = 0;
    let mut status_size = std::mem::size_of::<u32>() as u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut status as *mut u32 as *mut c_void),
        &mut status_size,
        std::ptr::null_mut(),
    )?;
    Ok(status)
}

/// Synchronous HTTP/1.1 POST of a small JSON body. The response body is
/// never read: only the status line matters, and a non-2xx/network error
/// is reported to the caller as an `Err` for logging.
fn post(host: &str, port: u16, path: &str, json_body: &str) -> Result<()> {
    unsafe {
        let connection = open(host, port, "POST", path)?;

        // Everything up to (excluding) the NUL terminator added by `wide`.
        let headers = wide("Content-Type: application/json\r\n");
        let headers = &headers[..headers.len() - 1];
        let body_bytes = json_body.as_bytes();
        WinHttpSendRequest(
            connection.request.0,
            Some(headers),
            Some(body_bytes.as_ptr().cast()),
            body_bytes.len() as u32,
            body_bytes.len() as u32,
            0,
        )?;
        WinHttpReceiveResponse(connection.request.0, std::ptr::null_mut())?;
        let status = status_code(connection.request.0)?;
        if !(200..300).contains(&status) {
            return Err(Error::new(E_FAIL, format!("HTTP {status}").into()));
        }
        Ok(())
    }
}

/// Maximum accepted response body: this only ever fetches a small, fixed
/// JSON object, never a file (mirrors the caps already used for
/// config.toml/credentials.dat).
const MAX_BODY_BYTES: usize = 16 * 1024;

enum HttpGet {
    Body(Vec<u8>),
    NotFound,
}

/// Synchronous HTTP/1.1 GET, returning the response body capped at
/// `MAX_BODY_BYTES`. Only 200 and 404 are accepted; any other status is a
/// hard error (network/server failure), unlike a 404 which is a normal,
/// expected outcome (e.g. a kiosk not yet configured on Manager).
fn get(host: &str, port: u16, path: &str) -> Result<HttpGet> {
    unsafe {
        let connection = open(host, port, "GET", path)?;
        WinHttpSendRequest(
            connection.request.0,
            None,
            None,
            0,
            0,
            0,
        )?;
        WinHttpReceiveResponse(connection.request.0, std::ptr::null_mut())?;
        let status = status_code(connection.request.0)?;
        if status == 404 {
            return Ok(HttpGet::NotFound);
        }
        if status != 200 {
            return Err(Error::new(E_FAIL, format!("HTTP {status}").into()));
        }

        let mut body = Vec::new();
        loop {
            let mut available: u32 = 0;
            WinHttpQueryDataAvailable(connection.request.0, &mut available)?;
            if available == 0 {
                break;
            }
            if body.len() + available as usize > MAX_BODY_BYTES {
                return Err(Error::new(E_FAIL, "réponse trop volumineuse".to_string().into()));
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read: u32 = 0;
            WinHttpReadData(
                connection.request.0,
                chunk.as_mut_ptr().cast(),
                available,
                &mut read,
            )?;
            if read == 0 {
                break;
            }
            chunk.truncate(read as usize);
            body.extend_from_slice(&chunk);
        }
        Ok(HttpGet::Body(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_config_response_parses_manager_json_and_ignores_extra_fields() {
        // `username` is served for symmetry but unused here: the Windows
        // account name is already the lookup key, not something to parse
        // back out of the response.
        let json = br#"{"enabled":true,"server":"192.168.10.50","port":3389,
            "username":"kiosk-noc-1","password":"s3cret","ignore_certificate_errors":true}"#;
        let parsed: RdpConfigResponse = serde_json::from_slice(json).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.server, "192.168.10.50");
        assert_eq!(parsed.port, 3389);
        assert_eq!(parsed.password.as_deref(), Some("s3cret"));
        assert!(parsed.ignore_certificate_errors);
    }

    #[test]
    fn rdp_config_response_defaults_missing_fields_when_disabled() {
        let parsed: RdpConfigResponse = serde_json::from_slice(br#"{"enabled":false}"#).unwrap();
        assert!(!parsed.enabled);
        assert_eq!(parsed.port, 3389);
        assert!(parsed.password.is_none());
        assert!(!parsed.ignore_certificate_errors);
    }
}
