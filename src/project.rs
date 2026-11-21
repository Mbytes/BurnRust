//! Project persistence: the document is serialised to JSON (`.rbp`).

use std::path::Path;

use anyhow::{Context, Result};

use crate::model::Document;

pub fn save(doc: &Document, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(doc).context("serializando documento")?;
    std::fs::write(path, json).with_context(|| format!("escribiendo {}", path.display()))?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Document> {
    let data = std::fs::read_to_string(path).with_context(|| format!("leyendo {}", path.display()))?;
    let doc: Document = serde_json::from_str(&data).context("parseando proyecto")?;
    Ok(doc)
}
