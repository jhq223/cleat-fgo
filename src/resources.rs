//! Immutable snapshots: hooks never wait for network or file downloads.
use crate::{
    bundle::{ScriptBundle, decode_text},
    data::Translations,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Resources {
    pub generation: u64,
    pub scripts: ScriptBundle,
    pub translations: Translations,
    pub localization: String,
    pub localization_values: std::collections::HashMap<String, String>,
    pub font_path: Option<PathBuf>,
    pub font_bytes: Option<std::sync::Arc<[u8]>>,
}
impl Resources {
    pub fn load(base: &Path, generation: u64) -> Self {
        let mut dirs = vec![base.join("cache/mappings")];
        for active in ["active-pack"] {
            if let Ok(id) = fs::read_to_string(base.join("cache").join(active)) {
                let id = id.trim();
                if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    dirs.push(base.join("cache/packs").join(id));
                }
            }
        }
        dirs.push(base.to_owned());
        dirs.push(base.join("overrides"));
        let translations = Translations::load(&dirs);
        let mut scripts = ScriptBundle::default();
        let mut localization = String::new();
        let mut font_path = None;
        for dir in &dirs {
            if let Err(error) = scripts.merge(ScriptBundle::load_dir(dir)) {
                log::warn!("stories: {error}");
            }
            let file = dir.join("LocalizationJpn.txt");
            if let Ok(bytes) = fs::read(&file) {
                match localization_text(&bytes) {
                    Ok(text) => localization = text,
                    Err(error) => log::warn!("localization: {}: {error}", file.display()),
                }
            }
            let font = dir.join("Font");
            if font.is_file() {
                font_path = Some(font);
            }
        }
        log::info!(
            "resources generation {generation}: {} translation entries, {} story scripts, localization={}, font={}",
            translations.len(),
            scripts.len(),
            !localization.is_empty(),
            font_path.is_some()
        );
        let font_bytes = font_path.as_ref().and_then(|path| {
            if fs::metadata(path).ok()?.len() > crate::bundle::MAX_BUNDLE {
                return None;
            }
            fs::read(path).ok().map(std::sync::Arc::from)
        });
        let localization_values = serde_json::from_str(&localization).unwrap_or_default();
        Self {
            generation,
            translations,
            scripts,
            localization,
            localization_values,
            font_path,
            font_bytes,
        }
    }
}
pub fn localization_text(bytes: &[u8]) -> Result<String, String> {
    let text = decode_text(bytes)?;
    if text.trim().is_empty() {
        return Err("empty localization resource".into());
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if !value.is_object() && !value.is_array() {
        return Err("localization must be a JSON object or array".into());
    }
    Ok(text)
}
