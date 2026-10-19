//! Display translation for NGUI, uGUI and TMP, with unchanged original fallback.
use crate::APP;
use cleat::prelude::*;
fn translated(text: Il2CppString) -> Il2CppString {
    if text.is_null() {
        return text;
    }
    let Some(app) = APP.get() else {
        return text;
    };
    let source = text.to_string_lossy();
    if let Some(cn) = app.resources.load().translations.ui(&source) {
        if let Ok(value) = Il2CppString::try_new(&cn) {
            app.translated
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return value;
        }
    } else {
        app.missing.record(&source);
    }
    text
}
#[cleat::hook("UILabel", "set_text")]
pub fn ngui(this: &Object, text: Il2CppString) -> Result<()> {
    ngui::original(this, translated(text))
}
#[cleat::hook("UnityEngine.UI.Text", "set_text")]
pub fn ugui(this: &Object, text: Il2CppString) -> Result<()> {
    ugui::original(this, translated(text))
}
#[cleat::hook("TMPro.TMP_Text", "set_text")]
pub fn tmp(this: &Object, text: Il2CppString) -> Result<()> {
    tmp::original(this, translated(text))
}
