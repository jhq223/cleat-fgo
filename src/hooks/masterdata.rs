use cleat::prelude::*;

use crate::obscured;
use crate::APP;
use crate::data::{Translations, normalize_width};

/// Strip FGO rich-text markup tags from game text before matching.
///
/// The game injects tags like `[g][o]▲[/o][/g]` into skill/treasure-device
/// descriptions that the Chaldea translation dataset does not contain.
/// Stripping these before lookup makes exact matching work.
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

// ── Dictionary raw-memory offsets (matches FGO's .NET runtime layout) ───
//
// Array length and data pointer are obtained through cleat's
// `Il2CppArray::len()` and `Il2CppArray::data_ptr()`; only the
// Dictionary-specific offsets remain below.

/// Offset of `entries` field in Dictionary<K,V> (64-bit).
/// Layout: klass(8) monitor(8) buckets(8) entries(8) count(4) ...
const DICT_OFF_ENTRIES: isize = 0x18;

/// Size of a Dictionary<K,V>::Entry in the entries array (64-bit).
/// Layout: hashCode(4) next(4) key_ptr(8) value_ptr(8)
const DICT_ENTRY_SIZE: usize = 24;
const DICT_ENTRY_OFF_VALUE: usize = 16;

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

    // ── ServantLimitAddMaster (per-ascension name overrides) ─────────
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
    let sample_seen: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    for i in 0..count {
        let Ok(item) = col.invoke_with::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };
        let Ok(jp) = item.load::<Il2CppString>(field) else {
            load_failed += 1;
            continue;
        };
        let raw_jp = jp.to_string_lossy();
        if raw_jp == "<null>" {
            empty += 1;
            continue;
        }
        if raw_jp == "<invalid UTF-16>" {
            invalid_utf16 += 1;
            continue;
        }

        let normalized = normalize_width(&raw_jp);
        let jp = strip_fgo_markup(&normalized);

        if let Some(cn) = t.get_any(categories, &jp) {
            if item.store(field, Il2CppString::new(cn)).is_ok() {
                replaced += 1;
                continue;
            }
        }
        // Missed: either no CN mapping or store failed
        missed += 1;
        if sample_missed.borrow().len() < 5 && sample_seen.borrow_mut().insert(jp.clone()) {
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
        let Some(raw_jp) = obscured::obscured_str(&obs) else {
            continue;
        };

        let normalized = normalize_width(&raw_jp);
        let jp = strip_fgo_markup(&normalized);

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

// ── ServantLimitAddMaster patch ───────────────────────────────────────────
//
// Mirrors FGOAssetsModifyTool's CommonUI__InitMaskClick where it walks
// `ServantLimitAddMaster.script` (a Dictionary<string, object>) and
// replaces any String value with a translation.
//
// Without this patch, per-ascension servant-name overrides remain in
// Japanese — so switching stages makes the name revert to JP.

fn patch_servant_limit_add(dm: &Il2CppObject, t: &Translations) -> cleat::Result<()> {
    let klass_name = "ServantLimitAddMaster";
    let (col, count) = fetch_master_data(dm, klass_name)?;

    let mut replaced = 0u32;
    let mut entries_checked = 0u32;

    for i in 0..count {
        let Ok(item) = col.invoke_with::<Il2CppObject>("get_Item", (i,)) else {
            continue;
        };

        // Load the `script` field — a Dictionary<string, object>
        let Ok(script) = item.load::<Il2CppObject>("script") else {
            continue;
        };
        if script.raw_ptr().is_null() {
            continue;
        }

        // Read Dictionary.entries pointer (offset 0x18 from dict base).
        let Some(entries_ptr) = (unsafe { script.read_ptr_at(DICT_OFF_ENTRIES) }) else {
            continue;
        };

        // Wrap as u8 array to access length and data pointer via cleat.
        let arr = unsafe { Il2CppArray::<u8>::from_raw(entries_ptr) };
        let entry_count = arr.len();
        if entry_count == 0 {
            continue;
        }
        let data = arr.data_ptr();

        for j in 0..entry_count {
            let entry = unsafe { data.add(j * DICT_ENTRY_SIZE) };
            // Read value pointer at offset 16 within each 24-byte Entry.
            // (Entry layout: hashCode(4) next(4) key_ptr(8) value_ptr(8))
            let val_ptr = unsafe {
                std::ptr::read::<*mut std::ffi::c_void>(
                    entry.add(DICT_ENTRY_OFF_VALUE) as *const _,
                )
            };
            if val_ptr.is_null() {
                continue;
            }

            entries_checked += 1;

            // Decode the value as a managed string.
            let jp = unsafe { Il2CppString::from_raw_ptr(val_ptr) }.to_string_lossy();
            if jp == "<null>" || jp == "<invalid UTF-16>" {
                continue;
            }

            let normalized = normalize_width(&jp);
            let jp_key = strip_fgo_markup(&normalized);

            // Mirror upstream: try svt_names first, then skill_names, td_names, td_ruby
            let Some(cn) = t.get_any(
                &["svt_names", "skill_names", "td_names", "td_ruby"],
                &jp_key,
            ) else {
                continue;
            };

            // Create a new Il2CppString for the translation and write its
            // pointer into the dictionary entry's value slot.
            let cn_string = Il2CppString::new(cn);
            let cn_raw = cn_string.as_ptr();
            let val_slot =
                unsafe { entry.add(DICT_ENTRY_OFF_VALUE) as *mut *mut std::ffi::c_void };
            unsafe { std::ptr::write(val_slot, cn_raw) };
            replaced += 1;
        }
    }

    log::debug!(
        "  {klass_name}.script: {replaced} replaced, {entries_checked} entries checked"
    );
    Ok(())
}
