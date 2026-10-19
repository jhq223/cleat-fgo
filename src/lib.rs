use arc_swap::ArcSwap;
#[cfg(debug_assertions)]
use std::sync::atomic::Ordering;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock, atomic::AtomicU64},
};
#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
mod apk_update;
mod bundle;
mod classes;
mod data;
mod hooks;
mod missing;
mod network;
mod obscured;
mod resources;
mod update;

pub(crate) struct AppContext {
    pub base: PathBuf,
    pub config: update::Config,
    pub resources: ArcSwap<resources::Resources>,
    pub missing: missing::Missing,
    pub translated: AtomicU64,
}
impl AppContext {
    pub fn reload(&self) {
        let generation = self.resources.load().generation + 1;
        self.resources
            .store(Arc::new(resources::Resources::load(&self.base, generation)));
    }
}
pub(crate) static APP: OnceLock<AppContext> = OnceLock::new();

#[cleat::main]
fn main() -> cleat::Result<()> {
    log::info!("cleat-fgo {} starting", env!("CARGO_PKG_VERSION"));
    let base =
        PathBuf::from("/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files/Mod");
    fs::create_dir_all(&base).map_err(|e| cleat::Error::Bridge(e.to_string()))?;
    let config = update::Config::load(&base);
    let missing = missing::Missing::new(&base, config.record_missing);
    let resources = ArcSwap::from_pointee(resources::Resources::default());
    APP.set(AppContext {
        base,
        config,
        resources,
        missing,
        translated: AtomicU64::new(0),
    })
    .map_err(|_| cleat::Error::Bridge("FGO module already initialized".into()))?;
    hooks::install();
    std::thread::spawn(|| {
        if let Some(app) = APP.get() {
            app.reload();
        }
        update::start();
    });
    #[cfg(debug_assertions)]
    {
        // Keep diagnostics local; no account data is read or sent.
        for name in [
            "Fgo.Ngui",
            "CodeStage",
            "UnityEngine.CoreModule",
            "UnityEngine.AssetBundleModule",
            "UnityEngine.TextRenderingModule",
            "System",
        ] {
            if let Some(assembly) = cleat::bridge::api::cache::assembly(name) {
                let _ = fs::write(
                    APP.get().unwrap().base.join(format!("debug-{name}.cs")),
                    assembly.to_string(),
                );
            }
        }
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                if let Some(app) = APP.get() {
                    log::info!(
                        "translation counters: {} UI replacements",
                        app.translated.load(Ordering::Relaxed)
                    );
                }
            }
        });
    }
    Ok(())
}
