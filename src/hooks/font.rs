use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use cleat::prelude::*;

use crate::APP;

// ── Font state ──
//
// Font loading is deferred to the first UILabel access because the Unity
// AssetBundle methods are not available during cleat::main().
//
//   FONT.get() == None         → not tried yet
//   FONT.get() == Some(None)   → tried, failed — pass through
//   FONT.get() == Some(Some()) → custom font loaded
//
// BACKUP stores the game's default font pointer (captured from the first
// UILabel that enters the hook). Only labels using this font are replaced;
// labels with other fonts (special effects, etc.) are left alone.
//
// REENTRY guards against a theoretical set_trueTypeFont → get_trueTypeFont loop.

static FONT: OnceLock<Option<Il2CppObject>> = OnceLock::new();
static BACKUP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static REENTRY: AtomicBool = AtomicBool::new(false);

#[cleat::hook("Assembly-CSharp", "UILabel", "get_trueTypeFont")]
fn font_hook(this: &Il2CppObject) -> cleat::Result<Il2CppObject> {
    if REENTRY.swap(true, Ordering::SeqCst) {
        return Ok(font_hook::original(this));
    }
    let result = font_hook_impl(this);
    REENTRY.store(false, Ordering::SeqCst);
    result
}

fn font_hook_impl(this: &Il2CppObject) -> cleat::Result<Il2CppObject> {
    // One-shot lazy-init: load the custom font once per process.
    let custom = match FONT.get_or_init(|| {
        let Some(ctx) = APP.get() else {
            log::error!("font: APP not set");
            return None;
        };
        match load_font(&ctx.font_path) {
            Ok(f) => {
                log::info!("font: loaded ({:p})", f.raw_ptr());
                Some(f)
            }
            Err(e) => {
                log::error!("font: {e}");
                None
            }
        }
    }) {
        Some(f) => f,
        None => return Ok(font_hook::original(this)),
    };

    // Only replace labels that use the game's default font.
    let current: Il2CppObject = match this.load("mTrueTypeFont") {
        Ok(f) => f,
        Err(_) => return Ok(font_hook::original(this)),
    };
    let cur_ptr = current.raw_ptr();

    // Already our custom font — nothing to do.
    if cur_ptr == custom.raw_ptr() {
        return Ok(font_hook::original(this));
    }

    // Capture the game default font on first access.
    let backup = BACKUP.load(Ordering::SeqCst);
    if backup.is_null() {
        BACKUP.store(cur_ptr, Ordering::SeqCst);
    }

    // Replace only if this label uses the default font.
    if backup.is_null() || cur_ptr == backup {
        let _ = this.invoke_void("set_trueTypeFont", (*custom,));
    }

    Ok(font_hook::original(this))
}

/// Loads the custom font file as a Unity Font object via AssetBundle.
fn load_font(path: &std::path::Path) -> cleat::Result<Il2CppObject> {
    let data =
        std::fs::read(path).map_err(|e| cleat::Error::Hook(format!("read font file: {e}")))?;

    cleat::load_asset_from_memory(
        &data,
        "FGO-Main-Font-Mod",
        "UnityEngine.Font",
        "UnityEngine.TextRenderingModule.dll",
    )
}
