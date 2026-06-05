use cleat::prelude::*;

use crate::APP;

/// Replaces decrypted script text with translations from `.script` bundles.
///
/// The asset name (first 2 chars in ASCII digit range: `0`–`9`, `:`, `;`, `<`,
/// `=`, `>`, `?`, `@`) identifies a script entry.  Lookup is against the
/// pre-loaded `ScriptBundle` (TextBundle format).
#[cleat::hook("Assembly-CSharp", "AssetData", "GetDecryptObjectText")]
fn script_hook(
    _this: &Il2CppObject,
    name: Il2CppString,
    key: Il2CppString,
) -> cleat::Result<Il2CppString> {
    // Mirror FGOAssetsModifyTool check: chars[0] in (0x29, 0x40)
    let raw = name.to_string_lossy();
    let first = raw.as_bytes().first().copied().unwrap_or(0);
    if first > 0x29
        && first < 0x40
        && let Some(cn) = APP.get().and_then(|ctx| ctx.scripts.get(&raw))
    {
        log::debug!("script: {raw}");
        return Ok(Il2CppString::new(cn));
    }
    Ok(script_hook::original(_this, name, key))
}
