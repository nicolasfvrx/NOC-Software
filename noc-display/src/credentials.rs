//! Configured password with optional user-scoped DPAPI storage.
use crate::config::RdpSettings;
use std::{io::Read, path::PathBuf};
use windows::{
    core::*,
    Win32::{Foundation::*, Security::Cryptography::*, System::Console::*},
};

pub struct Secret(pub Vec<u16>);
impl Drop for Secret {
    fn drop(&mut self) {
        for value in &mut self.0 {
            unsafe {
                std::ptr::write_volatile(value, 0);
            }
        }
    }
}
pub trait CredentialProvider {
    fn load_credentials(&self, target: &RdpSettings) -> Result<Secret>;
}
pub struct DpapiProvider;
pub struct ConfiguredProvider;
impl CredentialProvider for ConfiguredProvider {
    fn load_credentials(&self, target: &RdpSettings) -> Result<Secret> {
        match &target.password {
            Some(password) => Ok(Secret(password.encode_utf16().collect())),
            None => DpapiProvider.load_credentials(target),
        }
    }
}

#[cfg(test)]
#[test]
fn configured_password_takes_priority_without_dpapi() -> Result<()> {
    let target = RdpSettings {
        password: Some("synthetic-test-é\\value".into()),
        ..Default::default()
    };
    assert_eq!(
        ConfiguredProvider.load_credentials(&target)?.0,
        "synthetic-test-é\\value".encode_utf16().collect::<Vec<_>>()
    );
    Ok(())
}
fn failure(message: &str) -> Error {
    Error::new(E_FAIL, message.into())
}
fn entropy(target: &RdpSettings) -> Vec<u8> {
    format!(
        "DisplayClient/v1\0{}\0{}\0{}\0{}",
        target.server, target.port, target.domain, target.username
    )
    .into_bytes()
}
fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}
struct LocalBlob(CRYPT_INTEGER_BLOB);
impl Drop for LocalBlob {
    fn drop(&mut self) {
        unsafe {
            if !self.0.pbData.is_null() {
                for i in 0..self.0.cbData as usize {
                    std::ptr::write_volatile(self.0.pbData.add(i), 0);
                }
                let _ = LocalFree(HLOCAL(self.0.pbData.cast()));
            }
        }
    }
}
impl DpapiProvider {
    pub fn path() -> Result<PathBuf> {
        std::env::var_os("USERPROFILE")
            .filter(|p| !p.is_empty())
            .map(|p| {
                PathBuf::from(p)
                    .join(".display-client")
                    .join("credentials.dat")
            })
            .ok_or_else(|| failure("Profil utilisateur indisponible"))
    }
    fn encrypt(target: &RdpSettings, password: &Secret) -> Result<Vec<u8>> {
        if password.0.is_empty() || password.0.len() > 512 {
            return Err(failure("Longueur du mot de passe invalide"));
        }
        let context = entropy(target);
        unsafe {
            let input =
                std::slice::from_raw_parts(password.0.as_ptr().cast::<u8>(), password.0.len() * 2);
            let mut out = LocalBlob(CRYPT_INTEGER_BLOB::default());
            CryptProtectData(
                &blob(input),
                w!("NOC Display"),
                Some(&blob(&context)),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out.0,
            )?;
            Ok(std::slice::from_raw_parts(out.0.pbData, out.0.cbData as usize).to_vec())
        }
    }
    fn decrypt(target: &RdpSettings, encrypted: &[u8]) -> Result<Secret> {
        let context = entropy(target);
        unsafe {
            let mut out = LocalBlob(CRYPT_INTEGER_BLOB::default());
            CryptUnprotectData(
                &blob(encrypted),
                None,
                Some(&blob(&context)),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out.0,
            )?;
            if out.0.cbData == 0 || out.0.cbData > 1024 || out.0.cbData % 2 != 0 {
                return Err(failure("Identifiants DPAPI invalides"));
            }
            let data = std::slice::from_raw_parts(out.0.pbData, out.0.cbData as usize);
            Ok(Secret(
                data.chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect(),
            ))
        }
    }
    pub fn save(target: &RdpSettings, password: &Secret) -> Result<()> {
        let encrypted = Self::encrypt(target, password)?;
        let path = Self::path()?;
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|_| failure("Création du dossier credentials impossible"))?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, encrypted).map_err(|_| failure("Écriture DPAPI impossible"))?;
        std::fs::rename(temp, path).map_err(|_| failure("Enregistrement DPAPI impossible"))
    }
}
impl CredentialProvider for DpapiProvider {
    fn load_credentials(&self, target: &RdpSettings) -> Result<Secret> {
        let mut data = Vec::new();
        std::fs::File::open(Self::path()?)
            .map_err(|_| failure("credentials.dat absent ou illisible"))?
            .take(65_537)
            .read_to_end(&mut data)
            .map_err(|_| failure("Lecture DPAPI impossible"))?;
        if data.len() > 65_536 {
            return Err(failure("Fichier DPAPI trop volumineux"));
        }
        Self::decrypt(target, &data)
    }
}

/// Explicit provisioning mode, separate from the kiosk and from MsTscAx.
pub fn provision() -> Result<()> {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
    }
    let config = crate::config::Config::read().map_err(|e| failure(&e))?;
    if config.rdp.server.is_empty() || config.rdp.username.is_empty() {
        return Err(failure(
            "Renseigner server et username avant --set-credentials",
        ));
    }
    println!(
        "Enregistrement DPAPI pour le compte Windows courant. Le mot de passe ne sera pas affiché."
    );
    let first = read_password("Mot de passe RDP : ")?;
    let second = read_password("Confirmer : ")?;
    if first.0 != second.0 {
        return Err(failure("Les mots de passe ne correspondent pas"));
    }
    DpapiProvider::save(&config.rdp, &first)?;
    println!(
        "Identifiants chiffrés enregistrés dans {}",
        DpapiProvider::path()?.display()
    );
    Ok(())
}
fn read_password(prompt: &str) -> Result<Secret> {
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE)?;
        let mut mode = CONSOLE_MODE::default();
        GetConsoleMode(handle, &mut mode)?;
        struct Restore(HANDLE, CONSOLE_MODE);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    let _ = SetConsoleMode(self.0, self.1);
                }
            }
        }
        let _restore = Restore(handle, mode);
        SetConsoleMode(handle, (mode | ENABLE_LINE_INPUT) & !ENABLE_ECHO_INPUT)?;
        let mut secret = Secret(vec![0; 515]);
        let mut count = 0;
        ReadConsoleW(
            handle,
            secret.0.as_mut_ptr().cast(),
            secret.0.len() as u32,
            &mut count,
            None,
        )?;
        let mut end = count as usize;
        while end > 0 && matches!(secret.0[end - 1], 10 | 13) {
            end -= 1;
        }
        if end == 0 || end > 512 {
            return Err(failure("Longueur du mot de passe invalide"));
        }
        secret.0.truncate(end);
        println!();
        Ok(secret)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpapi_roundtrip_and_endpoint_binding() -> Result<()> {
        let mut target = RdpSettings {
            server: "test.invalid".into(),
            username: "test".into(),
            ..Default::default()
        };
        let password = Secret("synthetic-unit-test".encode_utf16().collect());
        let encrypted = DpapiProvider::encrypt(&target, &password)?;
        assert_eq!(DpapiProvider::decrypt(&target, &encrypted)?.0, password.0);
        target.server = "other.invalid".into();
        assert!(DpapiProvider::decrypt(&target, &encrypted).is_err());
        Ok(())
    }
}
