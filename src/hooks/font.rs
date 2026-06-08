use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use cleat::prelude::*;

use crate::APP;

// ── Font state ──
//
// Font loading is deferred to the first UILabel access because the Unity
// AssetBundle ICALLs are not available during cleat::main().
//
//   FONT.get() == None         → not tried yet
//   FONT.get() == Some(None)   → tried, failed — pass through
//   FONT.get() == Some(Some()) → custom font loaded
//
// BACKUP stores the game's default font pointer (captured from the first
// UILabel that enters the hook). Only labels using this font are replaced;
// labels with other fonts (special effects, etc.) are left alone.
//
// REENTRY guards against a theoretical set_trueTypeFont → get_trueTypeFont loop.

static FONT: OnceLock<Option<Il2CppObject>> = OnceLock::new();
static BACKUP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static REENTRY: AtomicBool = AtomicBool::new(false);

#[cleat::hook("Assembly-CSharp", "UILabel", "get_trueTypeFont")]
fn font_hook(this: &Il2CppObject) -> cleat::Result<Il2CppObject> {
    if REENTRY.swap(true, Ordering::SeqCst) {
        return Ok(font_hook::original(this));
    }
    let result = font_hook_impl(this);
    REENTRY.store(false, Ordering::SeqCst);
    result
}

fn font_hook_impl(this: &Il2CppObject) -> cleat::Result<Il2CppObject> {
    // One-shot lazy init: load the custom font once per process.
    let custom = match FONT.get_or_init(|| {
        let Some(ctx) = APP.get() else {
            log::error!("font: APP not set");
            return None;
        };
        match load_font(&ctx.font_path) {
            Ok(f) => {
                log::info!("font: loaded ({:p})", f.raw_ptr());
                Some(f)
            }
            Err(e) => {
                log::error!("font: {e}");
                None
            }
        }
    }) {
        Some(f) => f,
        None => return Ok(font_hook::original(this)),
    };

    // Only replace labels that use the game's default font.
    let current: Il2CppObject = match this.load("mTrueTypeFont") {
        Ok(f) => f,
        Err(_) => return Ok(font_hook::original(this)),
    };
    let cur_ptr = current.raw_ptr();

    // Already our custom font — nothing to do.
    if cur_ptr == custom.raw_ptr() {
        return Ok(font_hook::original(this));
    }

    // Capture the game default font on first access.
    let backup = BACKUP.load(Ordering::SeqCst);
    if backup.is_null() {
        BACKUP.store(cur_ptr, Ordering::SeqCst);
    }

    // Replace only if this label uses the default font.
    if backup.is_null() || cur_ptr == backup {
        let _ = this.invoke_void("set_trueTypeFont", (*custom,));
    }

    Ok(font_hook::original(this))
}

/// Loads the custom font file as a Unity Font object via AssetBundle ICALLs.
fn load_font(path: &std::path::Path) -> cleat::Result<Il2CppObject> {
    let data =
        std::fs::read(path).map_err(|e| cleat::Error::Hook(format!("read font file: {e}")))?;

    // Build a Unity byte[] from the raw file bytes.
    let byte_class = Il2CppClass::find("System.Byte")?;
    let mut arr = Il2CppArray::new(&byte_class, data.len())?;
    for (i, &b) in data.iter().enumerate() {
        unsafe { arr.set_unchecked(i, b) };
    }

    // Resolve the four ICALLs needed to load an asset from memory.
    let p_load = cleat::resolve_icall(
        "UnityEngine.AssetBundle::LoadFromMemoryAsync_Internal(System.Byte[],System.UInt32)",
    )?;
    let p_get_bundle =
        cleat::resolve_icall("UnityEngine.AssetBundleCreateRequest::get_assetBundle()")?;
    let p_load_asset = cleat::resolve_icall(
        "UnityEngine.AssetBundle::LoadAssetAsync_Internal(System.String,System.Type)",
    )?;
    let p_get_all = cleat::resolve_icall("UnityEngine.AssetBundleRequest::get_allAssets()")?;

    // 1. Create the asset bundle request from the byte array.
    let req = unsafe { cleat::invoke_icall_2(p_load, arr.as_ptr(), std::ptr::null_mut()) };
    if req.is_null() {
        return Err(cleat::Error::Hook(
            "LoadFromMemoryAsync returned null".into(),
        ));
    }
    let req = unsafe { Il2CppObject::from_raw(req) };

    // 2. Get the AssetBundle from the completed request.
    let bundle_ptr = unsafe { cleat::invoke_icall_1(p_get_bundle, req.raw_ptr()) };
    if bundle_ptr.is_null() {
        return Err(cleat::Error::Hook("get_assetBundle returned null".into()));
    }
    let bundle = unsafe { Il2CppObject::from_raw(bundle_ptr) };

    // 3. Create a dummy Font to obtain the System.Type for Font.
    let font_class =
        Il2CppClass::find_with("UnityEngine.Font", "UnityEngine.TextRenderingModule.dll")?;
    let dummy = font_class.new_object()?;
    let font_type: Il2CppObject = dummy.invoke::<Il2CppObject>("GetType", ())?;

    // 4. Load the font asset by name.
    let font_name = Il2CppString::new("FGO-Main-Font-Mod");
    let asset_req = unsafe {
        cleat::invoke_icall_3(
            p_load_asset,
            bundle.raw_ptr(),
            font_name.as_ptr(),
            font_type.raw_ptr(),
        )
    };
    if asset_req.is_null() {
        return Err(cleat::Error::Hook("LoadAssetAsync returned null".into()));
    }
    let asset_req = unsafe { Il2CppObject::from_raw(asset_req) };

    // 5. Get the loaded assets array.
    let all = unsafe { cleat::invoke_icall_1(p_get_all, asset_req.raw_ptr()) };
    if all.is_null() {
        return Err(cleat::Error::Hook("get_allAssets returned null".into()));
    }
    let all_arr = unsafe { Il2CppArray::<Il2CppObject>::from_raw(all) };

    all_arr
        .get(0)
        .ok_or_else(|| cleat::Error::Hook("no font asset in bundle".into()))
}
