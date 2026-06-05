use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Chaldea-format translation entry: `{ "jp_key": { "CN": "中文", ... }, ... }`.
#[derive(serde::Deserialize)]
struct TranslationEntry {
    #[serde(rename = "CN")]
    cn: Option<String>,
}

pub struct Translations {
    maps: HashMap<String, HashMap<String, String>>,
}

impl Translations {
    pub fn load(mod_path: impl AsRef<Path>) -> Self {
        let mod_path = mod_path.as_ref();
        let mut maps = HashMap::new();

        let categories = [
            "svt_names",
            "ce_names",
            "cc_names",
            "costume_names",
            "mc_names",
            "mc_detail",
            "skill_names",
            "skill_detail",
            "td_names",
            "td_ruby",
            "td_types",
            "td_detail",
            "item_names",
            "entity_names",
            "quest_names",
            "spot_names",
            "event_names",
            "war_names",
            "buff_names",
            "buff_detail",
            "event_mission",
            "mission_names",
        ];

        for name in categories {
            let path = mod_path.join(format!("{name}.json"));
            let Ok(json) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(raw) = serde_json::from_str::<HashMap<String, TranslationEntry>>(&json) else {
                log::warn!("translations: {name}.json parse failed");
                continue;
            };
            let m: HashMap<String, String> = raw
                .into_iter()
                .filter_map(|(k, v)| v.cn.map(|cn| (k, cn)))
                .collect();
            if !m.is_empty() {
                log::info!("  {name}: {} entries", m.len());
                maps.insert(name.to_string(), m);
            }
        }

        Self { maps }
    }

    pub fn get(&self, category: &str, key: &str) -> Option<&str> {
        self.maps.get(category)?.get(key).map(String::as_str)
    }

    /// Try each category in order (fallback chain), mirroring FGOAssetsModifyTool.
    pub fn get_any(&self, categories: &[&str], key: &str) -> Option<&str> {
        categories.iter().find_map(|cat| self.get(cat, key))
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }
}
