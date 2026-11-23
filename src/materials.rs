//! Material library: named cut-setting presets, persisted to the user's config dir.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{CutMode, CutSettings, Dither};

/// A saved material preset.
#[derive(Clone, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub thickness_mm: f64,
    pub notes: String,
    pub settings: CutSettings,
}

fn cut(power: f64, speed: f64, passes: u32) -> CutSettings {
    CutSettings {
        mode: CutMode::Line,
        power_pct: power,
        min_power_pct: 0.0,
        speed,
        passes,
        line_interval_mm: 0.1,
        dither: Dither::Grayscale,
        overscan_mm: 2.0,
    }
}

fn engrave(power: f64, speed: f64, interval: f64) -> CutSettings {
    CutSettings {
        mode: CutMode::Image,
        power_pct: power,
        min_power_pct: 0.0,
        speed,
        passes: 1,
        line_interval_mm: interval,
        dither: Dither::Grayscale,
        overscan_mm: 2.0,
    }
}

/// Built-in starter presets (tuned conservatively for a 20 W diode; always test!).
pub fn defaults() -> Vec<Material> {
    vec![
        Material {
            name: "Contrachapado 3 mm — corte".into(),
            thickness_mm: 3.0,
            notes: "2 pasadas, aire recomendado".into(),
            settings: cut(100.0, 300.0, 2),
        },
        Material {
            name: "MDF 3 mm — corte".into(),
            thickness_mm: 3.0,
            notes: "".into(),
            settings: cut(100.0, 250.0, 2),
        },
        Material {
            name: "Acrílico 3 mm — corte".into(),
            thickness_mm: 3.0,
            notes: "Solo acrílico fundido (cast)".into(),
            settings: cut(100.0, 200.0, 3),
        },
        Material {
            name: "Madera — grabado".into(),
            thickness_mm: 0.0,
            notes: "Escala de grises".into(),
            settings: engrave(35.0, 3000.0, 0.1),
        },
        Material {
            name: "Cuero — grabado".into(),
            thickness_mm: 0.0,
            notes: "".into(),
            settings: engrave(25.0, 4000.0, 0.12),
        },
        Material {
            name: "Cartón — corte".into(),
            thickness_mm: 2.0,
            notes: "".into(),
            settings: cut(60.0, 600.0, 1),
        },
    ]
}

/// `$HOME/.config/BurnRust/materials.json`.
pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("BurnRust");
    p.push("materials.json");
    Some(p)
}

/// Load the library, falling back to the built-in defaults.
pub fn load() -> Vec<Material> {
    if let Some(path) = config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<Material>>(&data) {
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }
    defaults()
}

/// Persist the library to disk.
pub fn save(materials: &[Material]) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "sin HOME",
        ));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(materials)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}
