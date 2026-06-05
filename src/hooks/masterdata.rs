use cleat::prelude::*;

use crate::obscured;
use crate::APP;
use crate::data::Translations;

// ── Mapping tables ──
//
// IL2CPP fields come in two flavours in FGO's MasterData:
//   * Plain Il2CppString   → load / store directly.
//   * ObscuredString       → pointer to an ObscuredString object; must
//     decrypt hiddenValue, replace, and re-encrypt.

type StringMapping = (&'static str, &'static str, &'static [&'static str]);

/// Plain `Il2CppString` fields — read/write via cleat's typed field
/// access.
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

/// An `ObscuredString*` mapping: `fields[0]` is the source field (used for
/// decryption + lookup); all fields (including `fields[0]`) are written back
/// with the same encrypted bytes.  This mirrors upstream FGOAssetsModifyTool
/// where, e.g., `CommandCodeMaster.ruby->hiddenValue` is set to the same
/// pointer as `name->hiddenValue`.
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

// ── Hook ──────────────────────────────────────────────────────────────────

#[cleat::hook("Assembly-CSharp", "CommonUI", "InitMaskClick")]
fn masterdata_hook(this: &Il2CppObject) -> cleat::Result<()> {
    let Some(ctx) = APP.get() else {
        masterdata_hook::original(this);
        return Ok(());
    };

    // Patch master data BEFORE original InitMaskClick so the game sees
    // translated data during UI initialisation (matches upstream FGOAssetsModifyTool).
    log::info!("masterdata: patching {} str + {} obs mappings...", STRING_MAPPINGS.len(), OBS_MAPPINGS.len());

    let dm: Il2CppObject = match Il2CppClass::find("DataManager")?.static_field_value("instance") {
        Ok(d) => d,
        Err(e) => {
            log::warn!("masterdata: DataManager not available: {e}");
            return Ok(());
        }
    };

    let mut ok = 0u32;
    let mut skip = 0u32;

    // ── Plain string fields ──────────────────────────────────────────
    for &(klass, field, cats) in STRING_MAPPINGS {
        match patch_string(&dm, klass, field, cats, &ctx.translations) {
            Ok(()) => ok += 1,
            Err(e) => {
                log::warn!("masterdata: {e}");
                skip += 1;
            }
        }
    }

    // ── ObscuredString fields ────────────────────────────────────────
    for m in OBS_MAPPINGS {
        match patch_obscured(&dm, m, &ctx.translations) {
            Ok(()) => ok += 1,
            Err(e) => {
                log::warn!("masterdata: {e}");
                skip += 1;
            }
        }
    }

    log::info!("masterdata: {ok} patched, {skip} skipped — now calling original InitMaskClick");
    masterdata_hook::original(this);
    Ok(())
}

// ── Patch helpers ────────────────────────────────────────────────────────

/// Fetch one MasterData<T> collection via `DataManager.GetMasterData<T>()`.
fn fetch_master_data(dm: &Il2CppObject, klass_name: &str) -> cleat::Result<(Il2CppObject, i32)> {
    let mkerr = |e: cleat::Error| cleat::Error::Hook(format!("{klass_name}: {e}"));

    let klass = Il2CppClass::find(klass_name).map_err(&mkerr)?;
    let method = dm.method("GetMasterData").map_err(&mkerr)?;
    let inflated = method.inflate(&[&klass]).map_err(&mkerr)?;
    let data: Il2CppObject = inflated.invoke(()).map_err(&mkerr)?;

    let col: Il2CppObject = data.load("list").map_err(&mkerr)?;
    let count: i32 = col.invoke("get_Count").map_err(&mkerr)?;

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

    let mut replaced = 0u32;
    let mut missed = 0u32;
    let mut empty = 0u32;
    let mut invalid_utf16 = 0u32;
    let mut load_failed = 0u32;
    let sample_missed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());

    for i in 0..count {
        let Ok(item) = col.invoke_with::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };
        let Ok(jp) = item.load::<Il2CppString>(field) else {
            load_failed += 1;
            continue;
        };
        let jp = jp.to_string_lossy();
        if jp == "<null>" {
            empty += 1;
            continue;
        }
        if jp == "<invalid UTF-16>" {
            invalid_utf16 += 1;
            continue;
        }
        if let Some(cn) = t.get_any(categories, &jp) {
            if item.store(field, Il2CppString::new(cn)).is_ok() {
                replaced += 1;
                continue;
            }
        }
        // Missed: either no CN mapping or store failed
        missed += 1;
        if sample_missed.borrow().len() < 5 {
            sample_missed.borrow_mut().push(jp);
        }
    }

    if missed > 0 {
        log::warn!(
            "  {klass_name}.{field}: {replaced} ok, {missed} missed, {empty} null, {invalid_utf16} bad-utf16, {load_failed} load-err"
        );
        for (j, s) in sample_missed.borrow().iter().enumerate() {
            log::warn!("    sample[{j}]: [{s}]");
        }
    } else {
        log::debug!("  {klass_name}.{field}: {replaced} replaced");
    }
    Ok(())
}

/// Patch one or more `ObscuredString*` fields per entry.
///
/// `mapping.fields[0]` is decrypted + looked up; the same CN value is then
/// encrypted and written to *every* field in `mapping.fields` (matching
/// upstream where `CommandCodeMaster.ruby->hiddenValue` gets the same
/// pointer as `name->hiddenValue`).
fn patch_obscured(
    dm: &Il2CppObject,
    mapping: &ObsMapping,
    t: &Translations,
) -> cleat::Result<()> {
    let (col, count) = fetch_master_data(dm, mapping.klass)?;

    let primary_field = mapping.fields[0];
    let mut replaced = 0u32;

    for i in 0..count {
        let Ok(item) = col.invoke_with::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };

        // Decrypt the primary field.
        let Ok(obs) = item.load::<Il2CppObject>(primary_field) else {
            continue;
        };
        let Some(jp) = obscured::obscured_str(&obs) else {
            continue;
        };
        let Some(cn) = t.get_any(mapping.categories, &jp) else {
            continue;
        };
        let cn = cn.to_string();

        // Re-encrypt and write back to all linked fields (same value).
        let mut ok = true;
        for &f in mapping.fields {
            let Ok(obs_f) = item.load::<Il2CppObject>(f) else {
                ok = false;
                continue;
            };
            if !obscured::obscured_replace(&obs_f, &cn) {
                ok = false;
            }
        }
        if ok {
            replaced += 1;
        }
    }

    log::debug!(
        "  {}.{:?}: {replaced} replaced",
        mapping.klass,
        mapping.fields
    );
    Ok(())
}
