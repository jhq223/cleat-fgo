# cleat-fgo

Fate/Grand Order (JP) translation mod — powered by [cleat](https://github.com/jhq223/cleat).

## Features

### Master Data Translation

Patches 21 in-game data tables at runtime via IL2CPP hooks:

| Category | Examples |
|---|---|
| Servants | Names (base + per-ascension overrides) |
| Skills, Noble Phantasms | Names, descriptions, ruby, types |
| Craft Essences, Command Codes | Names, descriptions |
| Costumes, Mystic Codes | Names, details |
| Items, Shop | Names |
| Quests, Maps, Events | Names |
| Buffs, Missions | Names, descriptions |

Handles FGO-specific `ObscuredString` encryption and strips rich-text markup
tags before translation lookup.

### Script Bundle Replacement

Decrypts `.script` bundle files (TextBundle: Rijndael-256-CBC → zlib → UTF-16LE)
and replaces in-game story/dialogue text.

### Class Name Translation

Translates servant class IDs to English (`Saber`, `Archer`, `Berserker`, …).

### Custom Font

Loads a Unity Font asset from the Mod folder and replaces the default font on
all `UILabel` instances.

### Custom Localization

Injects a `LocalizationJpn.txt` file into `LocalizationManager.SetTextData`.

## Mod Folder

Place under `/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files/Mod/`:

```
Mod/
├── svt_names.json
├── ce_names.json
├── cc_names.json
├── costume_names.json
├── mc_names.json / mc_detail.json
├── skill_names.json / skill_detail.json
├── td_names.json / td_ruby.json / td_types.json / td_detail.json
├── item_names.json
├── entity_names.json
├── quest_names.json / spot_names.json / event_names.json / war_names.json
├── buff_names.json / buff_detail.json
├── event_mission.json / mission_names.json
├── *.script              # TextBundle files
├── LocalizationJpn.txt   # UTF-16LE
└── Font                  # Unity Font asset bundle
```

Translation JSONs follow the [chaldea-data] format. Download them from
[chaldea-data/mappings](https://github.com/chaldea-center/chaldea-data/tree/main/mappings):

```json
{
  "アルトリア・ペンドラゴン": { "CN": "阿尔托莉雅·潘德拉贡" },
  "エミヤ": { "CN": "英灵卫宫" }
}
```

Keys are normalised (fullwidth → halfwidth) before matching.

[chaldea-data]: https://github.com/chaldea-center/chaldea-data

## Build

**Requirements**

- Rust nightly
- Android NDK r26+
- [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) ≥ 3.0

```bash
cargo ndk -t arm64-v8a -o ./target/jniLibs build --release
# → target/jniLibs/arm64-v8a/libcleat_fgo.so
```

## License

MIT
