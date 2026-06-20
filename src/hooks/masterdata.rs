use cleat::prelude::*;

use crate::APP;
use crate::data::{Translations, normalize_width};
use crate::obscured;

/// Strip FGO rich-text markup tags like `[g][o]▲[/o][/g]` before matching.
/// The translation dataset doesn't contain these, so stripping them makes exact matching work.
fn strip_fgo_markup(s: &str) -> String {
    // FGO markup tags use only ASCII characters (`[x]`, `[/x]`), so we can
    // detect them at the byte level without allocating a Vec<char>.
    let mut result = String::with_capacity(s.len());
    let mut skip = 0usize;
    for (i, c) in s.char_indices() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if c == '[' {
            let rest = &s[i..];
            let rb = rest.as_bytes();
            // Check [/x] — 4 ASCII bytes: '[' '/' lowercase ']'
            if rb.len() >= 4 && rb[1] == b'/' && rb[2].is_ascii_lowercase() && rb[3] == b']' {
                skip = 3;
                continue;
            }
            // Check [x] — 3 ASCII bytes: '[' lowercase ']'
            if rb.len() >= 3 && rb[1].is_ascii_lowercase() && rb[2] == b']' {
                skip = 2;
                continue;
            }
        }
        result.push(c);
    }
    result
}

type StringMapping = (&'static str, &'static str, &'static [&'static str]);

/// Plain Il2CppString fields.
const STRING_MAPPINGS: &[StringMapping] = &[
    ("ServantMaster", "battleName", &["svt_names"]),
    (
        "SkillMaster",
        "name",
        &["skill_names", "cc_names", "ce_names"],
    ),
    ("SkillDetailMaster", "detail", &["skill_detail"]),
    ("SkillDetailMaster", "detailShort", &["skill_detail"]),
    ("TreasureDvcMaster", "name", &["td_names"]),
    ("TreasureDvcMaster", "ruby", &["td_ruby"]),
    ("TreasureDvcMaster", "typeText", &["td_types"]),
    ("TreasureDvcDetailMaster", "detail", &["td_detail"]),
    ("TreasureDvcDetailMaster", "detailShort", &["td_detail"]),
    ("ItemMaster", "name", &["item_names"]),
    (
        "ShopMaster",
        "name",
        &["entity_names", "item_names", "ce_names", "cc_names"],
    ),
    ("SpotMaster", "name", &["spot_names"]),
    ("QuestMaster", "name", &["quest_names"]),
    ("EventMaster", "name", &["event_names"]),
    ("WarMaster", "name", &["war_names"]),
    ("ServantCostumeMaster", "name", &["costume_names"]),
    ("BuffMaster", "name", &["buff_names"]),
    ("BuffMaster", "detail", &["buff_detail"]),
    ("EquipMaster", "detail", &["mc_detail"]),
    ("EventMissionMaster", "name", &["event_mission"]),
    ("EventMissionMaster", "detail", &["buff_detail"]),
    (
        "EventMissionConditionMaster",
        "conditionMessage",
        &["event_mission"],
    ),
    ("SkillAddMaster", "name", &["skill_names"]),
];

/// ObscuredString mapping. `fields[0]` is used for decryption + lookup;
/// all fields get the same re-encrypted value. Mirrors upstream where
/// `CommandCodeMaster.ruby->hiddenValue` shares a pointer with `name->hiddenValue`.
struct ObsMapping {
    klass: &'static str,
    fields: &'static [&'static str],
    categories: &'static [&'static str],
}

const OBS_MAPPINGS: &[ObsMapping] = &[
    ObsMapping {
        klass: "CommandCodeMaster",
        fields: &["name", "ruby"],
        categories: &["cc_names"],
    },
    ObsMapping {
        klass: "EquipMaster",
        fields: &["name"],
        categories: &["mc_names"],
    },
    ObsMapping {
        klass: "ServantMaster",
        fields: &["nameSave"],
        categories: &["svt_names", "ce_names", "entity_names"],
    },
];

/// Dictionary<string,object> entry layout (64-bit IL2CPP).
/// We use Il2CppDictionary for typed access instead of raw offsets.
///
/// Entry size = 24 bytes: hashCode(4) + next(4) + key(8) + value(8)
/// Value offset within entry = 16
const DICT_ENTRY_SIZE: usize = 24;
const DICT_ENTRY_OFF_VALUE: usize = 16;

// NOTE: These constants are kept for the ServantLimitAddMaster script field
// which uses a raw Dictionary<string,object> accessed via memory offsets.
// Once Il2CppDictionary is proven on-device, this can be replaced too.

#[cleat::hook("Assembly-CSharp", "CommonUI", "InitMaskClick")]
fn masterdata_hook(this: &Il2CppObject) -> cleat::Result<()> {
    let Some(ctx) = APP.get() else {
        masterdata_hook::original(this);
        return Ok(());
    };

    log::info!(
        "masterdata: {} str + {} obs mappings",
        STRING_MAPPINGS.len(),
        OBS_MAPPINGS.len()
    );

    let dm: Il2CppObject = match Il2CppClass::find("DataManager")?.static_field_value("instance") {
        Ok(d) => d,
        Err(e) => {
            log::warn!("masterdata: DataManager not available: {e}");
            return Ok(());
        }
    };

    let mut ok = 0u32;
    let mut skip = 0u32;

    for &(klass, field, cats) in STRING_MAPPINGS {
        match patch_string(&dm, klass, field, cats, &ctx.translations) {
            Ok(()) => ok += 1,
            Err(e) => {
                log::warn!("masterdata: {e}");
                skip += 1;
            }
        }
    }

    for m in OBS_MAPPINGS {
        match patch_obscured(&dm, m, &ctx.translations) {
            Ok(()) => ok += 1,
            Err(e) => {
                log::warn!("masterdata: {e}");
                skip += 1;
            }
        }
    }

    match patch_servant_limit_add(&dm, &ctx.translations) {
        Ok(()) => ok += 1,
        Err(e) => {
            log::warn!("masterdata: {e}");
            skip += 1;
        }
    }

    log::info!("masterdata: {ok} patched, {skip} skipped — now calling original InitMaskClick");
    masterdata_hook::original(this);
    Ok(())
}

/// Fetch a MasterData<T> collection.
fn fetch_master_data(dm: &Il2CppObject, klass_name: &str) -> cleat::Result<(Il2CppObject, i32)> {
    let mkerr = |e: cleat::Error| cleat::Error::Hook(format!("{klass_name}: {e}"));

    let klass = Il2CppClass::find(klass_name).map_err(&mkerr)?;
    let method = dm.method("GetMasterData").map_err(&mkerr)?;
    let inflated = method.inflate(&[&klass]).map_err(&mkerr)?;
    let data: Il2CppObject = inflated.invoke(()).map_err(&mkerr)?;

    let col: Il2CppObject = data.load("list").map_err(&mkerr)?;
    let count: i32 = col.invoke::<i32>("get_Count", ()).map_err(&mkerr)?;

    Ok((col, count))
}

/// Patch a plain `Il2CppString` field.
fn patch_string(
    dm: &Il2CppObject,
    klass_name: &str,
    field: &str,
    categories: &[&str],
    t: &Translations,
) -> cleat::Result<()> {
    let (col, count) = fetch_master_data(dm, klass_name)?;

    for i in 0..count {
        let Ok(item) = col.invoke::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };
        let Ok(jp) = item.load::<Il2CppString>(field) else {
            continue;
        };
        let raw_jp = jp.to_string_lossy();
        if raw_jp == "<null>" || raw_jp == "<invalid UTF-16>" {
            continue;
        }

        let normalized = normalize_width(&raw_jp);
        let jp = strip_fgo_markup(&normalized);

        if let Some(cn) = t.get_any(categories, &jp) {
            let _ = item.store(field, Il2CppString::new(cn));
        }
    }
    Ok(())
}

/// Patch ObscuredString fields. Decrypts `fields[0]`, looks up the
/// translation, then re-encrypts and writes to all fields in the mapping.
fn patch_obscured(dm: &Il2CppObject, mapping: &ObsMapping, t: &Translations) -> cleat::Result<()> {
    let (col, count) = fetch_master_data(dm, mapping.klass)?;

    let primary_field = mapping.fields[0];

    for i in 0..count {
        let Ok(item) = col.invoke::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };

        let Ok(obs) = item.load::<Il2CppObject>(primary_field) else {
            continue;
        };
        let Some(raw_jp) = obscured::obscured_str(&obs) else {
            continue;
        };

        let normalized = normalize_width(&raw_jp);
        let jp = strip_fgo_markup(&normalized);

        let Some(cn) = t.get_any(mapping.categories, &jp) else {
            continue;
        };
        let cn = cn.to_string();

        for &f in mapping.fields {
            let Ok(obs_f) = item.load::<Il2CppObject>(f) else {
                continue;
            };
            obscured::obscured_replace(&obs_f, &cn);
        }
    }
    Ok(())
}

/// Patch ServantLimitAddMaster.script (Dictionary<string,object>).
/// Without this, per-ascension servant names stay in Japanese.
fn patch_servant_limit_add(dm: &Il2CppObject, t: &Translations) -> cleat::Result<()> {
    let klass_name = "ServantLimitAddMaster";
    let (col, count) = fetch_master_data(dm, klass_name)?;

    for i in 0..count {
        let Ok(item) = col.invoke::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };

        // Load the script field directly as a Dictionary — it IS the
        // Dictionary<string,object>, not an object containing one.
        let dict: Il2CppDictionary<*mut std::ffi::c_void, *mut std::ffi::c_void> =
            match item.load("script") {
                Ok(d) => d,
                Err(_) => continue,
            };

        let Some(entries_arr) = dict.entries_array() else {
            continue;
        };
        let entry_count = entries_arr.len();
        if entry_count == 0 {
            continue;
        }
        let data = entries_arr.data_ptr();

        for j in 0..entry_count {
            let entry = unsafe { data.add(j * DICT_ENTRY_SIZE) };
            let val_ptr = unsafe {
                std::ptr::read::<*mut std::ffi::c_void>(entry.add(DICT_ENTRY_OFF_VALUE) as *const _)
            };
            if val_ptr.is_null() {
                continue;
            }

            let jp = unsafe { Il2CppString::from_raw_ptr(val_ptr) }.to_string_lossy();
            if jp == "<null>" || jp == "<invalid UTF-16>" {
                continue;
            }

            let normalized = normalize_width(&jp);
            let jp_key = strip_fgo_markup(&normalized);

            let Some(cn) = t.get_any(
                &["svt_names", "skill_names", "td_names", "td_ruby"],
                &jp_key,
            ) else {
                continue;
            };

            let cn_string = Il2CppString::new(cn);
            let cn_raw = cn_string.as_ptr();
            let val_slot = unsafe { entry.add(DICT_ENTRY_OFF_VALUE) as *mut *mut std::ffi::c_void };
            unsafe { std::ptr::write(val_slot, cn_raw) };
        }
    }
    Ok(())
}
