//! One coroutine tick per rendered Unity frame, even with multiple UI roots.
use cleat::prelude::*;
use std::sync::{
    LazyLock,
    atomic::{AtomicI32, Ordering},
};

pub static TASKS: LazyLock<CoroutineScheduler> = LazyLock::new(CoroutineScheduler::new);
static FRAME: AtomicI32 = AtomicI32::new(-1);

#[cleat::hook("UIRoot", "Update")]
pub fn update(this: &Object) -> Result<()> {
    update::original(this)?;
    let frame: i32 = Class::find("UnityEngine.Time")?.get("frameCount")?;
    if FRAME.swap(frame, Ordering::Relaxed) != frame {
        super::performance::on_main_thread();
        super::masterdata::tick();
        TASKS.tick()?;
    }
    Ok(())
}
