use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn read_utf16_txt(path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    // UTF-16 LE → Rust String (discard BOM if present)
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&u16s))
}

mod bundle;
mod data;
mod hooks;
pub(crate) mod obscured;

use bundle::ScriptBundle;
use data::Translations;

pub(crate) struct AppContext {
    pub(crate) scripts: ScriptBundle,
    pub(crate) translations: Translations,
    pub(crate) localization: String,
    pub(crate) font_path: PathBuf,
}

pub(crate) static APP: OnceLock<AppContext> = OnceLock::new();

#[cleat::main]
fn main() -> cleat::Result<()> {
    log::info!("cleat_fgo v{}", env!("CARGO_PKG_VERSION"));

    cleat::set_app_data("/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files");

    let base = cleat::app_data()?.join("Mod");
    fs::create_dir_all(&base).ok();
    log::info!("mod path: {}", base.display());

    let ctx = AppContext {
        scripts: ScriptBundle::load_dir(&base).unwrap_or_else(|e| {
            log::warn!("scripts: {e}");
            ScriptBundle::empty()
        }),
        translations: Translations::load(&base),
        localization: read_utf16_txt(&base.join("LocalizationJpn.txt")).unwrap_or_default(),
        font_path: base.join("Font"),
    };

    log::info!(
        "scripts: {}, translations: {} categories",
        ctx.scripts.len(),
        ctx.translations.len()
    );
    if !ctx.localization.is_empty() {
        log::info!("localization: {} chars", ctx.localization.len());
    }

    APP.set(ctx)
        .map_err(|_| cleat::Error::Hook("APP already set".into()))?;

    log::info!("installing hooks...");
    hooks::install()
}
