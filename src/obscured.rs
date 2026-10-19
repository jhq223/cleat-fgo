//! Game-owned ObscuredString conversions; no private field offsets or XOR copy.
use cleat::prelude::*;

pub fn obscured_str(object: &Object) -> Option<String> {
    if object.is_null() {
        return None;
    }
    object
        .call::<Il2CppString>("ToString", ())
        .ok()
        .map(|s| s.to_string_lossy())
}
/// Replace the field with a new managed value via the game's implicit operator.
/// This preserves the current game's encryption and validation bookkeeping.
pub fn set_field(owner: &Object, field: &str, text: &str) -> Result<()> {
    let old: Object = owner.get(field)?;
    if old.is_null() {
        return Err(Error::NullObject);
    }
    let class = old.class()?;
    let value: Object = class.call(("op_Implicit", ["System.String"]), (text,))?;
    owner.set(field, value)
}
