//! Capture assetbundle keys from `CatAndMouseGame.SetAssetbundleKeys`.
//! Needed by the story script decrypter to unpack encrypted scenario data.

use cleat::prelude::*;
use serde::Serialize;

const DICT_OFF_ENTRIES: isize = 0x18;
const DICT_OFF_COUNT: isize = 0x20;
const DICT_ENTRY_SIZE: usize = 24;
const DICT_ENTRY_OFF_KEY: usize = 8;
const DICT_ENTRY_OFF_VALUE: usize = 16;

#[derive(Serialize)]
struct KeyEntry {
    id: String,
    decrypt_key: String,
}

#[cleat::hook("Assembly-CSharp", "CatAndMouseGame", "SetAssetbundleKeys")]
fn set_assetbundle_keys_hook(obj: Il2CppObject) -> cleat::Result<()> {
    dump_keys(&obj);
    set_assetbundle_keys_hook::original(obj);
    Ok(())
}

fn dump_keys(list_obj: &Il2CppObject) {
    log::info!("assetbundle keys: SetAssetbundleKeys");

    let Some(items_ptr) = (unsafe { list_obj.read_ptr_at(0x10) }) else {
        log::warn!("  no _items pointer");
        return;
    };
    if (items_ptr as usize) < 0x10000 {
        log::warn!("  _items pointer too low: {items_ptr:p}");
        return;
    }

    let arr = unsafe { Il2CppArray::<Il2CppObject>::from_raw(items_ptr) };
    let len = arr.len();

    let mut entries = Vec::with_capacity(len.min(2000));

    for i in 0..len {
        let Some(dict_obj) = arr.get(i) else { continue };
        let dict_ptr = dict_obj.raw_ptr();
        if dict_ptr.is_null() { continue; }

        let count = unsafe {
            std::ptr::read::<i32>((dict_ptr as *const u8).offset(DICT_OFF_COUNT) as *const i32)
        };
        if count <= 0 || count > 100 { continue; }

        let entries_ptr = unsafe {
            std::ptr::read::<*mut std::ffi::c_void>(
                (dict_ptr as *const u8).offset(DICT_OFF_ENTRIES) as *const _
            )
        };
        if entries_ptr.is_null() || (entries_ptr as usize) < 0x10000 { continue; }

        let ent_arr = unsafe { Il2CppArray::<u8>::from_raw(entries_ptr) };
        let ent_data = ent_arr.data_ptr();

        let mut id = String::new();
        let mut decrypt_key = String::new();

        for j in 0..count as usize {
            let ent = unsafe { ent_data.add(j * DICT_ENTRY_SIZE) };
            let key_ptr = unsafe {
                std::ptr::read::<*mut std::ffi::c_void>(ent.add(DICT_ENTRY_OFF_KEY) as *const _)
            };
            let val_ptr = unsafe {
                std::ptr::read::<*mut std::ffi::c_void>(ent.add(DICT_ENTRY_OFF_VALUE) as *const _)
            };

            let key_str = if key_ptr.is_null() {
                String::new()
            } else {
                unsafe { Il2CppString::from_raw_ptr(key_ptr) }.to_string_lossy()
            };
            let val_str = if val_ptr.is_null() {
                String::new()
            } else {
                unsafe { Il2CppString::from_raw_ptr(val_ptr) }.to_string_lossy()
            };

            match key_str.as_str() {
                "id" => id = val_str,
                "decryptKey" => decrypt_key = val_str,
                other => log::debug!("  [{i}] unknown key: {other} = {val_str}"),
            }
        }

        if !id.is_empty() {
            entries.push(KeyEntry { id, decrypt_key });
        }
    }

    log::info!("  {} keys", entries.len());

    let out_path = std::path::PathBuf::from(
        "/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files/Mod",
    )
    .join("keys.json");
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&out_path, &json) {
                log::error!("  failed to write keys.json: {e}");
            } else {
                log::info!("  saved {} entries to {}", entries.len(), out_path.display());
            }
        }
        Err(e) => log::error!("  JSON serialize error: {e}"),
    }
}
