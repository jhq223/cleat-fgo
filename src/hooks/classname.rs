use cleat::prelude::*;

/// Translates FGO servant class IDs to English names.
#[cleat::hook("Assembly-CSharp", "ServantEntity", "getClassName")]
fn classname_hook(this: &Il2CppObject) -> cleat::Result<Il2CppString> {
    let id: i32 = this.load("classId")?;
    Ok(Il2CppString::new(class_name(id)))
}

fn class_name(id: i32) -> &'static str {
    match id {
        1 => "Saber",
        2 => "Archer",
        3 => "Lancer",
        4 => "Rider",
        5 => "Caster",
        6 => "Assassin",
        7 => "Berserker",
        8 => "Shielder",
        9 => "Ruler",
        10 => "Alterego",
        11 => "Avenger",
        17 => "Grand Caster",
        20 => "Beast II",
        22 => "Beast I",
        23 => "MoonCancer",
        24 => "Beast Ⅲ/R",
        25 => "Foreigner",
        26 => "Beast Ⅲ/L",
        27 => "Beast Unknown",
        28 => "Pretender",
        29 => "Beast Ⅳ",
        33 | 38 => "Beast",
        40 => "UnBeast",
        1000 => "OTHER",
        1001 => "ALL",
        1002 => "EXTRA",
        _ => "?",
    }
}
