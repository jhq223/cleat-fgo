use std::sync::atomic::{AtomicBool, Ordering};

use cleat::prelude::*;

use crate::APP;

/// Injects custom localization JSON into the game's LocalizationManager,
/// replacing the original SetTextData call exactly once.
static INJECTED: AtomicBool = AtomicBool::new(false);

#[cleat::hook("Assembly-CSharp", "LocalizationManager", "SetTextData")]
fn localization_hook(this: &Il2CppObject, _text: Il2CppString) -> cleat::Result<()> {
    if INJECTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let Some(ctx) = APP.get() else {
        return Ok(());
    };

    if ctx.localization.is_empty() {
        return Ok(());
    }

    log::info!("localization: injecting {} bytes", ctx.localization.len());
    localization_hook::original(this, Il2CppString::new(&ctx.localization));
    Ok(())
}
