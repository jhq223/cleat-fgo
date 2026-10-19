use cleat::prelude::*;

#[cleat::hook("ServantEntity", "getClassName")]
pub fn classname_hook(this: &Object) -> Result<Il2CppString> {
    let original = classname_hook::original(this)?;
    let Some(app) = crate::APP.get() else {
        return Ok(original);
    };
    let Ok(id) = this.get::<i32>("classId") else {
        return Ok(original);
    };
    let resources = app.resources.load();
    let Some(name) = resources.translations.class_name(id) else {
        return Ok(original);
    };
    // A missing/new class keeps the game's name, never becomes "?".
    Ok(Il2CppString::try_new(name).unwrap_or(original))
}
