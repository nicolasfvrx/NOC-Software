use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::models::{Kiosk, PasswordAction};

/// In-memory kiosk list, backed by a JSON file.
pub struct Storage {
    path: PathBuf,
    kiosks: Mutex<Vec<Kiosk>>,
}

impl Storage {
    /// Loads the JSON file, creating an empty one if it does not exist.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            std::fs::write(path, "[]\n")
                .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
            println!("[NOC Manager] created empty {}", path.display());
        }

        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let kiosks: Vec<Kiosk> = if raw.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&raw)
                .map_err(|e| format!("invalid {}: {e}", path.display()))?
        };

        Ok(Self {
            path: path.to_path_buf(),
            kiosks: Mutex::new(kiosks),
        })
    }

    pub fn all(&self) -> Vec<Kiosk> {
        self.kiosks.lock().unwrap().clone()
    }

    pub fn get(&self, username: &str) -> Option<Kiosk> {
        self.kiosks
            .lock()
            .unwrap()
            .iter()
            .find(|k| k.username == username)
            .cloned()
    }

    pub fn add(&self, kiosk: Kiosk) -> Result<(), String> {
        let mut kiosks = self.kiosks.lock().unwrap();
        crate::models::validate(&kiosk, &kiosks, None)?;
        kiosks.push(kiosk);
        Self::write(&self.path, &kiosks)
    }

    /// `password` resolves under the same lock as the lookup/write, so a
    /// concurrent update can never race with "keep the existing password".
    pub fn update(
        &self,
        previous_username: &str,
        mut kiosk: Kiosk,
        password: PasswordAction,
    ) -> Result<(), String> {
        let mut kiosks = self.kiosks.lock().unwrap();
        let index = kiosks
            .iter()
            .position(|k| k.username == previous_username)
            .ok_or_else(|| format!("kiosk '{previous_username}' not found"))?;
        kiosk.rdp.password = match password {
            PasswordAction::Keep => kiosks[index].rdp.password.clone(),
            PasswordAction::Set(value) => Some(value),
            PasswordAction::Clear => None,
        };
        crate::models::validate(&kiosk, &kiosks, Some(previous_username))?;
        kiosks[index] = kiosk;
        Self::write(&self.path, &kiosks)
    }

    pub fn delete(&self, username: &str) -> Result<(), String> {
        let mut kiosks = self.kiosks.lock().unwrap();
        let before = kiosks.len();
        kiosks.retain(|k| k.username != username);
        if kiosks.len() == before {
            return Err(format!("kiosk '{username}' not found"));
        }
        Self::write(&self.path, &kiosks)
    }

    /// Atomic-ish write: serialize to `<file>.tmp`, then rename over the target.
    fn write(path: &Path, kiosks: &[Kiosk]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(kiosks)
            .map_err(|e| format!("cannot serialize kiosks: {e}"))?;

        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);

        std::fs::write(&tmp, json.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        // std::fs::rename replaces the destination on both Windows and Unix.
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
        Ok(())
    }
}
