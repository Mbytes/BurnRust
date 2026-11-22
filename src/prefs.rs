//! Lightweight user preferences that persist across sessions: the last-used
//! machine configuration, the canvas background (light/dark) and the text font.
//! Stored as JSON in the user's config dir, like the material/machine libraries.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::MachineSettings;

/// Remembered "last selections", restored on the next launch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// White (light) canvas/app theme instead of the default dark one.
    #[serde(default)]
    pub canvas_light: bool,
    /// The active machine configuration (bed size, origin, S max, …).
    #[serde(default)]
    pub machine: Option<MachineSettings>,
    /// Selected text font family (empty = automatic).
    #[serde(default)]
    pub text_font: String,
}

/// `$HOME/.config/rustburn/prefs.json`.
pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("rustburn");
    p.push("prefs.json");
    Some(p)
}

/// Load saved preferences (defaults if missing / unreadable).
pub fn load() -> Prefs {
    if let Some(path) = config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(p) = serde_json::from_str::<Prefs>(&data) {
                return p;
            }
        }
    }
    Prefs::default()
}

/// Persist preferences to disk.
pub fn save(p: &Prefs) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "sin HOME"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(p)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}
