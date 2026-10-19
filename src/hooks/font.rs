//! Font loading uses Unity's async APIs and the frame-driven Rust scheduler.
use crate::APP;
use cleat::prelude::*;
use std::{cell::RefCell, path::PathBuf, sync::Arc};

#[derive(Default)]
struct FontState {
    generation: u64,
    path: Option<PathBuf>,
    loading: bool,
    font: Option<GcHandle>,
    original: Option<GcHandle>,
    previous: Vec<GcHandle>,
}
thread_local! { static STATE: RefCell<FontState> = RefCell::new(FontState::default()); }

#[cleat::hook("UILabel", "get_trueTypeFont")]
pub fn font_hook(this: &Object) -> Result<Object> {
    let original = font_hook::original(this)?;
    if original.is_null() {
        return Ok(original);
    }
    STATE
        .with(|state| {
            let Ok(mut state) = state.try_borrow_mut() else {
                return Ok(original);
            };
            let Some(app) = APP.get() else {
                return Ok(original);
            };
            let resources = app.resources.load();
            if !state.loading && state.generation != resources.generation {
                state.generation = resources.generation;
                if (state.font.is_none() || state.path != resources.font_path)
                    && let Some(bytes) = resources.font_bytes.clone()
                {
                    state.loading = true;
                    let path = resources.font_path.clone();
                    super::main_thread::TASKS.spawn(async move {
                        let result = load_font(bytes).await;
                        let success = result.is_ok();
                        STATE.with(|state| {
                            let mut state = state.borrow_mut();
                            state.loading = false;
                            match result {
                                Ok(font) => {
                                    if let Some(old) = state.font.replace(font) {
                                        state.previous.push(old);
                                    }
                                    state.path = path;
                                    log::info!("font: Chinese font loaded asynchronously");
                                }
                                Err(e) => log::warn!("font: {e}; preserving existing font"),
                            }
                        });
                        if success {
                            refresh_labels()?;
                        }
                        Ok(())
                    });
                }
            }
            let Some(custom) = state.font.as_ref().map(GcHandle::object) else {
                return Ok(original);
            };
            if original.raw_ptr() == custom.raw_ptr() {
                return Ok(original);
            }
            if state.original.is_none() {
                let name: Il2CppString = original.get("name").unwrap_or_default();
                if name.to_string_lossy() != "FGO-Main-Font" {
                    return Ok(original);
                }
                state.original = Some(original.root()?);
            }
            if state
                .original
                .iter()
                .chain(state.previous.iter())
                .any(|font| font.object().raw_ptr() == original.raw_ptr())
            {
                this.set("trueTypeFont", custom)?;
                return Ok(custom);
            }
            Ok(original)
        })
        .or_else(|error: Error| {
            log::warn!("font: {error}");
            Ok(original)
        })
}

async fn load_font(bytes: Arc<[u8]>) -> Result<GcHandle> {
    let bundle = AssetBundle::load_from_memory_async(&bytes).await?;
    let result = bundle
        .load_asset_async("FGO-Main-Font-Mod", "UnityEngine.Font")
        .await
        .and_then(|font| font.root());
    bundle.unload(false)?;
    result
}

pub fn refresh_labels() -> Result<()> {
    let labels = cleat::unity::find_objects("UILabel", true)?;
    let _root = labels.as_object().root()?;
    for label in labels.iter() {
        let _: Object = label.get("trueTypeFont")?;
        let text: Il2CppString = label.get("text")?;
        if let Some(app) = APP.get()
            && let Some(translated) = app
                .resources
                .load()
                .translations
                .ui(&text.to_string_lossy())
        {
            label.set("text", translated)?;
        }
    }
    log::info!("font: refreshed {} existing labels", labels.len());
    Ok(())
}
