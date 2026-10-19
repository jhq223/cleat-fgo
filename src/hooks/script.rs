use crate::APP;
use cleat::prelude::*;
#[cleat::hook("AssetData", "GetDecryptObjectText")]
pub fn script_hook(this: &Object, name: Il2CppString, key: Il2CppString) -> Result<Il2CppString> {
    let id = name.to_string_lossy();
    if let Some(app) = APP.get() {
        let resources = app.resources.load();
        if let Some(text) = resources.scripts.get(&id) {
            match Il2CppString::try_new(text) {
                Ok(value) => {
                    log::info!("story translated: {id}");
                    return Ok(value);
                }
                Err(e) => log::warn!("story {id}: {e}"),
            }
        }
    }
    script_hook::original(this, name, key)
}
