//! Class IDs are separate from text keys in Chaldea's enums.json.
//! Source: https://github.com/chaldea-center/chaldea-data/blob/main/mappings/enums.json
use std::collections::HashMap;

pub fn parse(bytes: &[u8]) -> Result<HashMap<i32, String>, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let mut names = HashMap::new();
    if let Some(classes) = value.get("svt_class").and_then(|v| v.as_object()) {
        for (id, entry) in classes {
            let Ok(id) = id.parse() else {
                continue;
            };
            // CN is authoritative. Grand classes currently only have NA labels.
            let cn = entry
                .get("CN")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            let grand = entry
                .get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.starts_with("grand"));
            let name = cn.or_else(|| {
                grand
                    .then(|| entry.get("NA").and_then(|v| v.as_str()))
                    .flatten()
            });
            if let Some(name) = name.filter(|s| !s.trim().is_empty()) {
                names.insert(id, name.to_owned());
            }
        }
    }
    Ok(names)
}

pub fn fallback(id: i32) -> Option<&'static str> {
    Some(match id {
        1 => "Saber",
        2 => "Archer",
        3 => "Lancer",
        4 => "Rider",
        5 => "Caster",
        6 => "Assassin",
        7 => "Berserker",
        8 => "Shielder",
        9 => "Ruler",
        10 => "Alterego",
        11 => "Avenger",
        12 => "魔神柱",
        17 | 10005 => "Grand Caster",
        20 => "Beast II",
        22 => "Beast I",
        23 => "MoonCancer",
        24 => "Beast Ⅲ/R",
        25 => "Foreigner",
        26 => "Beast Ⅲ/L",
        27 => "Beast Unknown",
        28 => "Pretender",
        29 => "Beast Ⅳ",
        30 => "Beast I?",
        33 | 38 => "Beast",
        34 => "Beast Ⅵ",
        35 => "Beast Ⅵ（魔兽）",
        40 => "UnBeast",
        42 | 43 => "Beast Ⅶ",
        44 => "DEMIBEAST",
        1000 => "OTHER",
        1001 => "ALL",
        1002 => "EXTRA",
        1003 => "MIX",
        1004 => "EXTRA1",
        1005 => "EXTRA2",
        1006 => "推荐助战",
        10001 => "Grand Saber",
        10002 => "Grand Archer",
        10003 => "Grand Lancer",
        10004 => "Grand Rider",
        10006 => "Grand Assassin",
        10007 => "Grand Berserker",
        10008 => "Grand Shielder",
        10009 => "Grand Ruler",
        10010 => "Grand Alterego",
        10011 => "Grand Avenger",
        10023 => "Grand MoonCancer",
        10025 => "Grand Foreigner",
        10028 => "Grand Pretender",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_classes_and_unknown_ids() {
        assert_eq!(fallback(43), Some("Beast Ⅶ"));
        assert_eq!(fallback(44), Some("DEMIBEAST"));
        assert_eq!(fallback(10007), Some("Grand Berserker"));
        assert_eq!(fallback(987654), None);
        let names = parse(br#"{"svt_class":{"777":{"CN":"New class"},"10001":{"CN":null,"NA":"Grand Saber","name":"grandSaber"},"31":{"CN":null,"name":"internal"}}}"#).unwrap();
        assert_eq!(names.get(&777).unwrap(), "New class");
        assert_eq!(names.get(&10001).unwrap(), "Grand Saber");
        assert!(!names.contains_key(&31));
    }
}
