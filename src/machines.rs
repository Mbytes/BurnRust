//! User-saved laser/machine profiles, persisted to the user's config dir.
//!
//! The built-in presets in the config window only set bed size and S max. A
//! profile here stores the *whole* [`MachineSettings`] (origin, laser mode,
//! baud, travel speed…) under a name, so the user can snapshot a fully-tuned
//! machine and restore it in one click.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::MachineSettings;

/// A named machine profile = a full machine configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct MachineProfile {
    pub name: String,
    pub settings: MachineSettings,
}

/// `$HOME/.config/BurnRust/machines.json`.
pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("BurnRust");
    p.push("machines.json");
    Some(p)
}

/// Load the saved profiles (empty list if none / unreadable).
pub fn load() -> Vec<MachineProfile> {
    if let Some(path) = config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<MachineProfile>>(&data) {
                return list;
            }
        }
    }
    Vec::new()
}

/// Persist the profiles to disk.
pub fn save(profiles: &[MachineProfile]) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "sin HOME"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(profiles)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}
