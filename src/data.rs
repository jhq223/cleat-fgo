//! Chaldea mappings, local overrides and conservative UI matching.
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::LazyLock,
};

pub const CATEGORIES: &[&str] = &[
    "bgm_names",
    "buff_detail",
    "buff_names",
    "cc_names",
    "ce_names",
    "chara_names",
    "costume_detail",
    "costume_names",
    "cv_names",
    "entity_names",
    "event_names",
    "func_popuptext",
    "illustrator_names",
    "item_names",
    "mc_detail",
    "mc_names",
    "mission_names",
    "quest_names",
    "skill_detail",
    "skill_names",
    "spot_names",
    "summon_names",
    "svt_names",
    "td_detail",
    "td_names",
    "td_ruby",
    "td_types",
    "voice_line_names",
    "war_names",
    "misc",
    "enums",
    "event_trait",
    "field_trait",
    "trait",
];

pub fn normalize_width(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

pub fn parse_mapping(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let raw: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let mut entries = HashMap::new();
    let mut ambiguous = HashSet::new();
    fn visit(
        key: &str,
        value: &serde_json::Value,
        entries: &mut HashMap<String, String>,
        ambiguous: &mut HashSet<String>,
    ) {
        let Some(object) = value.as_object() else {
            return;
        };
        if object.contains_key("CN") {
            let Some(cn) = object.get("CN").and_then(|v| v.as_str()) else {
                return;
            };
            let jp = object
                .get("JP")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(key);
            // Numeric keys in nested trait/enumeration maps identify records,
            // not displayed text. They must never translate unrelated numbers.
            if jp.bytes().all(|b| b.is_ascii_digit()) {
                return;
            }
            if jp.is_empty() || cn.trim().is_empty() || !placeholders_match(jp, cn) {
                return;
            }
            let jp = normalize_width(jp);
            if let Some(old) = entries.insert(jp.clone(), cn.to_owned())
                && old != cn
            {
                ambiguous.insert(jp);
            }
        } else {
            for (key, value) in object {
                visit(key, value, entries, ambiguous);
            }
        }
    }
    for (key, value) in &raw {
        visit(key, value, &mut entries, &mut ambiguous);
    }
    for key in ambiguous {
        entries.remove(&key);
    }
    Ok(entries)
}

// Reject translations which lose formatting arguments. Translators may reorder
// indexed placeholders; compare multisets, not their positions.
static PLACEHOLDERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\{\d+(?:[^{}]*)\}|%(?:\d+\$)?[-+0 #]*\d*(?:\.\d+)?[sdfi]|\[[a-zA-Z_][a-zA-Z_0-9]*\]",
    )
    .unwrap()
});
pub(crate) fn placeholders_match(source: &str, translated: &str) -> bool {
    let tokens = |s: &str| {
        let mut parts: Vec<String> = PLACEHOLDERS
            .find_iter(s)
            .map(|m| m.as_str().to_owned())
            .collect();
        parts.sort();
        parts
    };
    tokens(source) == tokens(translated)
}

#[derive(Default)]
pub struct Translations {
    maps: HashMap<String, HashMap<String, String>>,
    unique: HashMap<String, String>,
    ui: HashMap<String, String>,
    classes: HashMap<i32, String>,
}
impl Translations {
    /// Sources are ordered from lowest to highest priority. Local files win.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut result = Self {
            ui: serde_json::from_str(include_str!("ui.json")).expect("bundled UI translations"),
            ..Self::default()
        };
        for dir in dirs {
            if let Ok(bytes) = fs::read(dir.join("enums.json")) {
                match crate::classes::parse(&bytes) {
                    Ok(names) => result.classes.extend(names),
                    Err(error) => log::warn!("class translations: {error}"),
                }
            }
            for category in CATEGORIES.iter().copied().chain(["event_mission"]) {
                let path = dir.join(format!("{category}.json"));
                if !path.is_file() {
                    continue;
                }
                match fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| parse_mapping(&bytes))
                {
                    Ok(entries) => result
                        .maps
                        .entry(category.into())
                        .or_default()
                        .extend(entries),
                    Err(e) => log::warn!("translations: {}: {e}", path.display()),
                }
            }
            let path = dir.join("ui.json");
            if path.is_file() {
                match fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| parse_ui(&b))
                {
                    Ok(entries) => result.ui.extend(entries),
                    Err(e) => log::warn!("translations: {}: {e}", path.display()),
                }
            }
        }
        let mut ambiguous = HashSet::new();
        for entries in result.maps.values() {
            for (jp, cn) in entries {
                if let Some(old) = result.unique.insert(jp.clone(), cn.clone())
                    && old != *cn
                {
                    ambiguous.insert(jp.clone());
                }
            }
        }
        for key in ambiguous {
            result.unique.remove(&key);
        }
        result
    }
    pub fn get_any(&self, categories: &[&str], key: &str) -> Option<&str> {
        categories
            .iter()
            .find_map(|cat| self.maps.get(*cat)?.get(key).map(String::as_str))
    }
    pub fn translate(&self, categories: &[&str], source: &str) -> Option<String> {
        self.match_text(source, |key| self.get_any(categories, key))
    }
    pub fn ui(&self, source: &str) -> Option<String> {
        self.match_text(source, |key| {
            self.ui
                .get(key)
                .or_else(|| self.unique.get(key))
                .map(String::as_str)
        })
    }
    pub fn class_name(&self, id: i32) -> Option<&str> {
        self.classes
            .get(&id)
            .map(String::as_str)
            .or_else(|| crate::classes::fallback(id))
    }
    fn match_text<'a>(
        &'a self,
        source: &str,
        lookup: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<String> {
        if source.is_empty() {
            return None;
        }
        let key = normalize_width(source);
        if let Some(value) = lookup(&key) {
            return (value != source).then(|| value.to_owned());
        }
        // Preserve whitespace and whole-string rich-text wrappers. Internal
        // markup needs an explicit translation; stripping it changes meaning.
        let trimmed = key.trim();
        if let Some(value) = lookup(trimmed) {
            let start = key.len() - key.trim_start().len();
            return Some(format!(
                "{}{}{}",
                &key[..start],
                value,
                &key[start + trimmed.len()..]
            ));
        }
        let (prefix, inner, suffix) = wrappers(source);
        if inner != source {
            return lookup(&normalize_width(inner)).map(|value| format!("{prefix}{value}{suffix}"));
        }
        None
    }
    pub fn len(&self) -> usize {
        self.maps.values().map(HashMap::len).sum::<usize>() + self.ui.len()
    }
}

pub fn parse_ui(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let entries: HashMap<String, String> =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .filter(|(jp, cn)| !jp.is_empty() && !cn.trim().is_empty() && placeholders_match(jp, cn))
        .map(|(jp, cn)| (normalize_width(&jp), cn))
        .collect())
}

fn wrappers(source: &str) -> (&str, &str, &str) {
    let mut start = 0;
    let mut end = source.len();
    loop {
        let text = &source[start..end];
        let (left, right) = match text.as_bytes().first() {
            Some(b'[') => ('[', ']'),
            Some(b'<') => ('<', '>'),
            _ => break,
        };
        let Some(close) = text.find(right) else {
            break;
        };
        let tag = text[1..close].split('=').next().unwrap_or("");
        if tag.is_empty() || !tag.bytes().all(|b| b.is_ascii_lowercase()) {
            break;
        }
        let ending = format!("{left}/{tag}{right}");
        if !text.ends_with(&ending) || close + 1 + ending.len() > text.len() {
            break;
        }
        start += close + 1;
        end -= ending.len();
    }
    (&source[..start], &source[start..end], &source[end..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_markup_placeholders_and_unknown_text() {
        let t = Translations::load(&[]);
        assert_eq!(t.ui("[b]キャンセル[/b]").as_deref(), Some("[b]取消[/b]"));
        assert_eq!(
            t.ui("<color=#ffffff>キャンセル</color>").as_deref(),
            Some("<color=#ffffff>取消</color>")
        );
        assert!(t.ui("未知の固有名詞").is_none());
        assert!(placeholders_match("{0} / {1}", "{1} / {0}"));
        assert!(!placeholders_match("{0} / {1}", "{0}"));
        assert_eq!(normalize_width("Ａ＋Ｂ　Ｘ"), "A+B X");
    }
    #[test]
    fn empty_and_invalid_entries_do_not_replace_originals() {
        let entries = parse_mapping(
            br#"{"a":{"CN":""},"b":{"CN":null},"{0}":{"CN":"lost"},"ok":{"CN":"yes"}}"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries["ok"], "yes");
    }
    #[test]
    fn nested_mappings_use_japanese_values_and_preserve_named_arguments() {
        let entries = parse_mapping(r#"{"SkillScript":{"template":{"JP":"[servantName]の宝具","CN":"[servantName]的宝具"},"bad":{"JP":"[servantName]の宝具","CN":"宝具"}}}"#.as_bytes()).unwrap();
        assert_eq!(entries["[servantName]の宝具"], "[servantName]的宝具");
        assert!(!entries.contains_key("template"));
    }
    #[test]
    fn local_overrides_and_ambiguous_global_matches() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("svt_names.json"),
            r#"{"同じ":{"CN":"从者"}}"#,
        )
        .unwrap();
        fs::write(
            root.path().join("item_names.json"),
            r#"{"同じ":{"CN":"道具"}}"#,
        )
        .unwrap();
        let t = Translations::load(&[root.path().to_owned()]);
        assert!(t.ui("同じ").is_none());
        assert_eq!(t.get_any(&["svt_names"], "同じ"), Some("从者"));
        fs::write(root.path().join("ui.json"), r#"{"同じ":"指定译文"}"#).unwrap();
        assert_eq!(
            Translations::load(&[root.path().to_owned()])
                .ui("同じ")
                .as_deref(),
            Some("指定译文")
        );
    }
}
