mod assetbundle_keys;
mod classname;
mod font;
mod localization;
mod masterdata;
mod script;

use assetbundle_keys::{add_assetbundle_keys_hook, set_assetbundle_keys_hook};
use classname::classname_hook;
use font::font_hook;
use localization::localization_hook;
use masterdata::masterdata_hook;
use script::script_hook;

pub fn install() -> cleat::Result<()> {
    macro_rules! install {
        ($hook:ident) => {
            match $hook::install() {
                Ok(()) => log::info!("  ✓ {}", stringify!($hook)),
                Err(e) => {
                    log::error!("  ✗ {}: {e}", stringify!($hook));
                    return Err(e);
                }
            }
        };
    }

    install!(script_hook);
    install!(font_hook);
    install!(localization_hook);
    install!(masterdata_hook);
    install!(classname_hook);
    install!(set_assetbundle_keys_hook);
    install!(add_assetbundle_keys_hook);

    log::info!("all hooks installed");
    Ok(())
}
