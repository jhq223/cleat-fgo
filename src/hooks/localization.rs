use crate::APP;
use cleat::prelude::*;

// Some games initialize localization before metadata discovery finishes.
// Reading by key also covers that path and newly downloaded translations.
#[cleat::hook("LocalizationManager", "Get", ["System.String"])]
pub fn get(key: Il2CppString) -> Result<Il2CppString> {
    let original = get::original(key)?;
    let Some(app) = APP.get() else {
        return Ok(original);
    };
    let resources = app.resources.load();
    if let Some(value) = resources.localization_values.get(&key.to_string_lossy())
        && !value.is_empty()
        && crate::data::placeholders_match(&original.to_string_lossy(), value)
    {
        return Il2CppString::try_new(value);
    }
    Ok(original)
}

#[cleat::hook("LocalizationManager", "SetTextData")]
pub fn localization_hook(this: &Object, text: Il2CppString) -> Result<()> {
    let translated = APP.get().and_then(|app| {
        let resources = app.resources.load();
        // Always merge custom keys over the game's current table. A partial
        // localization file must never remove untranslated keys from the game.
        let mut original: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&text.to_string_lossy()).ok()?;
        let mut replacements = 0;
        for value in original.values_mut() {
            if let Some(source) = value.as_str()
                && let Some(cn) = resources.translations.ui(source)
            {
                *value = cn.into();
                replacements += 1;
            }
        }
        if !resources.localization.is_empty()
            && let Ok(custom) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                &resources.localization,
            )
        {
            for (key, value) in custom {
                if let (Some(source), Some(target)) =
                    (original.get(&key).and_then(|v| v.as_str()), value.as_str())
                    && !target.is_empty()
                    && crate::data::placeholders_match(source, target)
                {
                    original.insert(key, value);
                    replacements += 1;
                }
            }
        }
        if replacements == 0 {
            return None;
        }
        log::info!("localization: {replacements} translated keys");
        Il2CppString::try_new(&serde_json::to_string(&original).ok()?).ok()
    });
    localization_hook::original(this, translated.unwrap_or(text))
}
