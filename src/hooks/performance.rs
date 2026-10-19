//! Always use 60 FPS; there is deliberately no frame-rate setting.
use cleat::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
static APPLIED: AtomicBool = AtomicBool::new(false);
#[cleat::hook("UnityEngine.Application", "set_targetFrameRate")]
pub fn frame_rate(_value: i32) -> Result<()> {
    frame_rate::original(60)
}
pub fn on_main_thread() {
    if APPLIED.swap(true, Ordering::AcqRel) {
        return;
    }
    match Class::find("UnityEngine.Application").and_then(|app| app.set("targetFrameRate", 60i32)) {
        Ok(()) => log::info!("frame rate: 60 FPS"),
        Err(error) => log::warn!("frame rate: {error}"),
    }
}
