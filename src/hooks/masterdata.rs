//! Master data translation through managed fields and collection methods.
use crate::{APP, data::Translations, obscured};
use cleat::prelude::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};
static READY: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

type Mapping = (&'static str, &'static str, &'static [&'static str]);
const STRINGS: &[Mapping] = &[
    (
        "ServantMaster",
        "battleName",
        &["svt_names", "entity_names"],
    ),
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
    ("ServantCostumeMaster", "detail", &["costume_detail"]),
    ("BuffMaster", "name", &["buff_names"]),
    ("BuffMaster", "detail", &["buff_detail"]),
    ("EquipMaster", "detail", &["mc_detail"]),
    (
        "EventMissionMaster",
        "name",
        &["mission_names", "event_mission"],
    ),
    (
        "EventMissionMaster",
        "detail",
        &["mission_names", "event_mission"],
    ),
    (
        "EventMissionConditionMaster",
        "conditionMessage",
        &["mission_names", "event_mission"],
    ),
    ("SkillAddMaster", "name", &["skill_names"]),
];
const OBSCURED: &[Mapping] = &[
    ("CommandCodeMaster", "name", &["cc_names"]),
    ("CommandCodeMaster", "ruby", &["cc_names"]),
    ("EquipMaster", "name", &["mc_names"]),
    (
        "ServantMaster",
        "name",
        &["svt_names", "ce_names", "entity_names"],
    ),
    (
        "ServantMaster",
        "nameSave",
        &["svt_names", "ce_names", "entity_names"],
    ),
];
#[cleat::hook("CommonUI", "InitMaskClick")]
pub fn masterdata_hook(this: &Object) -> Result<()> {
    masterdata_hook::original(this)?;
    READY.store(true, Ordering::Relaxed);
    tick();
    Ok(())
}
pub fn tick() {
    let Some(app) = APP.get() else {
        return;
    };
    let generation = app.resources.load().generation;
    if !READY.load(Ordering::Relaxed)
        || generation == 0
        || generation == GENERATION.load(Ordering::Relaxed)
        || RUNNING.swap(true, Ordering::Relaxed)
    {
        return;
    }
    super::main_thread::TASKS.spawn(async move {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                RUNNING.store(false, Ordering::Relaxed);
            }
        }
        let _reset = Reset;
        if let Err(error) = apply().await {
            log::warn!("masterdata: {error}");
        }
        GENERATION.store(generation, Ordering::Relaxed);
        Ok(())
    });
}
async fn apply() -> Result<()> {
    let Some(app) = APP.get() else {
        return Ok(());
    };
    let resources = app.resources.load_full();
    let manager: Object = Class::find("DataManager")?.get("instance")?;
    if manager.is_null() {
        return Ok(());
    }
    let mut groups = BTreeMap::<&str, Vec<(&str, &[&str], bool)>>::new();
    for &(class, field, categories) in STRINGS {
        groups
            .entry(class)
            .or_default()
            .push((field, categories, false));
    }
    for &(class, field, categories) in OBSCURED {
        groups
            .entry(class)
            .or_default()
            .push((field, categories, true));
    }
    let _manager_root = manager.root()?;
    let mut budget = Instant::now();
    let mut replacements = 0;
    let mut failed = 0;
    for (name, fields) in groups {
        let result: Result<()> = async {
            let collection = fetch(&manager, name)?;
            let _keep = collection.as_object().root()?;
            let items = collection.snapshot()?;
            for item in items.iter() {
                if budget.elapsed() >= Duration::from_millis(2) {
                    cleat::coroutine::next_frame().await;
                    budget = Instant::now();
                }
                if item.is_null() {
                    continue;
                }
                for &(field, categories, encrypted) in &fields {
                    let source = if encrypted {
                        item.get::<Object>(field)
                            .ok()
                            .and_then(|value| obscured::obscured_str(&value))
                    } else {
                        item.get::<Il2CppString>(field)
                            .ok()
                            .filter(|v| !v.is_null())
                            .map(|v| v.to_string_lossy())
                    };
                    let Some(source) = source else {
                        continue;
                    };
                    if let Some(cn) = resources.translations.translate(categories, &source) {
                        let result = if encrypted {
                            obscured::set_field(&item, field, &cn)
                        } else {
                            item.set(field, cn)
                        };
                        match result {
                            Ok(()) => replacements += 1,
                            Err(e) => {
                                failed += 1;
                                if failed < 6 {
                                    log::warn!("{name}.{field}: {e}");
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        .await;
        if let Err(e) = result {
            log::warn!("masterdata {name}: {e}");
        }
    }
    match servant_overrides(&manager, &resources.translations).await {
        Ok(count) => replacements += count,
        Err(error) => log::warn!("servant overrides: {error}"),
    }
    log::info!(
        "masterdata: {replacements} strings translated, {failed} writes failed (resources {})",
        resources.generation
    );
    Ok(())
}
fn fetch(manager: &Object, name: &str) -> Result<Il2CppList<Object>> {
    let class = Class::find(name)?;
    let data: Object = manager
        .method(("GetMasterData", 0usize))?
        .generic(&[&class])?
        .invoke(())?;
    let collection: Object = data.get("list")?;
    Il2CppList::try_from(collection)
}
async fn servant_overrides(manager: &Object, translations: &Translations) -> Result<usize> {
    let collection = fetch(manager, "ServantLimitAddMaster")?;
    let _keep = collection.as_object().root()?;
    let mut count = 0;
    let mut budget = Instant::now();
    let items = collection.snapshot()?;
    for item in items.iter() {
        if budget.elapsed() >= Duration::from_millis(2) {
            cleat::coroutine::next_frame().await;
            budget = Instant::now();
        }
        let object: Object = item.get("script")?;
        if object.is_null() {
            continue;
        }
        let dictionary = Il2CppDictionary::<Il2CppString, Object>::try_from(object)?;
        // Finish enumeration before changing the dictionary's version.
        let mut changes = Vec::new();
        for entry in dictionary.iter()? {
            let (key, value) = entry?;
            let Ok(string) = Il2CppString::try_from(value) else {
                continue;
            };
            if let Some(cn) = translations.translate(
                &["svt_names", "skill_names", "td_names", "td_ruby"],
                &string.to_string_lossy(),
            ) {
                changes.push((key.to_string_lossy(), cn));
            }
        }
        for (key, cn) in changes {
            let value = Il2CppString::try_new(&cn)?;
            dictionary.set(key, value.as_object())?;
            count += 1;
        }
    }
    Ok(count)
}
