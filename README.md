# cleat-fgo

An Android translation module for FGO JP, built with [cleat](https://github.com/jhq223/cleat).

[中文](README_zh.md) · [Download APK](https://github.com/jhq223/cleat-fgo-build/releases/latest) · [Changes](CHANGELOG.md)

## Install

Download and install `fgo-mod.apk`, then start the game. The module enables 60 FPS and downloads translations automatically. Restart after the first download to apply story, interface and font updates.

[Chaldea](https://github.com/chaldea-center/chaldea-data) mappings provide translations for servants, skills, Noble Phantasms, equipment, items, quests and events. Story packs align CN dialogue, choices and speakers with JP scripts.

## Updates

The module checks for translation and APK updates every 24 hours. Story resources come from this repository's [resources Release](https://github.com/jhq223/cleat-fgo/releases/tag/resources), and APKs come from [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build/releases/latest).

Open the APK from the Android download notification when it is ready. Restart the game after translation updates.

## Import into Chaldea

After a successful login, the response is saved to:

```text
/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files/Mod/exports/chaldea-login.json
```

Select this file in Chaldea to import servant and material data. See the [Chaldea import guide](https://docs.chaldea.center/guide/import_https/). Each successful login updates the file.

## Configuration

Configuration is created on first launch at `Android/data/com.aniplex.fategrandorder/files/Mod/config.json`. Restart the game after editing it.

| Setting | Default | Purpose |
|---|---|---|
| `auto_update` | `true` | Update translation mappings and resource packs |
| `update_interval_hours` | `24` | Check interval, from 1 to 168 hours |
| `resource_manifest_url` | `manifest.json` in this repository's resources Release | Story resource URL; set to `null` to disable |
| `github_mirror` | `https://gh-proxy.com` | GitHub download mirror; set to `null` for direct access |
| `record_missing` | `true` | Record untranslated text in `missing-text.jsonl` |
| `export_login_response` | `true` | Save the Chaldea import file |
| `check_apk_updates` | `true` | Check for APK updates |
| `download_apk_updates` | `true` | Download new APKs automatically |

## Add translations

Place custom translations in `Mod/overrides/`. For example, create `ui.json`:

```json
{
  "ゲーム開始": "开始游戏",
  "閉じる": "关闭"
}
```

Each key is the original text and each value is its translation. Preserve format arguments such as `{0}` and `%s`.

| File | Contents |
|---|---|
| `ui.json` | Original-to-translated text mapping |
| `*.script` | Story bundle |
| `LocalizationJpn.txt` | Interface localization |
| `Font` | Unity AssetBundle containing `FGO-Main-Font-Mod` |
| Chaldea mapping JSON | Translations for the corresponding category |

Files load in this priority order: `overrides/` → the `Mod/` directory → online resources. Remove a local file to use its online version.

See [cleat-fgo-helper](https://github.com/jhq223/cleat-fgo-helper) to build and publish story packs.

## Build

Place cleat next to this repository and follow its [build guide](https://github.com/jhq223/cleat/blob/main/docs/en/BUILD.md) to set up Rust and Android NDK. Run:

```sh
cargo ndk -t arm64-v8a build --release --locked
```

The library is written to `target/aarch64-linux-android/release/libcleat_fgo.so`. See [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build) for APK packaging.