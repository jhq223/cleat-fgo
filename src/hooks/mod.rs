mod classname;
mod font;
mod localization;
mod login;
mod main_thread;
mod masterdata;
mod performance;
mod script;
mod text;

pub fn install() {
    type Installer = fn() -> cleat::Result<()>;
    let hooks: &[(&str, Installer)] = &[
        ("story", script::script_hook::install),
        ("main thread", main_thread::update::install),
        ("Chaldea login export", login::response_text::install),
        ("NGUI text", text::ngui::install),
        ("uGUI text", text::ugui::install),
        ("TMP text", text::tmp::install),
        ("Chinese font", font::font_hook::install),
        ("localization", localization::localization_hook::install),
        ("localization lookup", localization::get::install),
        ("master data", masterdata::masterdata_hook::install),
        ("class names", classname::classname_hook::install),
        ("60 FPS", performance::frame_rate::install),
    ];
    let mut installed = 0;
    for (name, install) in hooks {
        match install() {
            Ok(()) => {
                installed += 1;
                log::info!("hook installed: {name}");
            }
            Err(cleat::Error::ClassNotFound(_)) if *name == "TMP text" => {
                log::info!("TMP text: this game does not include TextMeshPro");
            }
            Err(error) => log::warn!("hook unavailable: {name}: {error}"),
        }
    }
    log::info!("hooks: {installed}/{} installed", hooks.len());
}
