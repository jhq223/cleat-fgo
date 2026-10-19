# cleat-fgo

FGO 日服 Android 汉化模块，基于 [cleat](https://github.com/jhq223/cleat)。

[English](README.md) · [下载 APK](https://github.com/jhq223/cleat-fgo-build/releases/latest) · [修改历史](CHANGELOG.md)

## 安装

下载并安装 `fgo-mod.apk`，启动游戏。模块默认启用 60 FPS，并自动下载翻译数据。首次下载完成后重启游戏，应用剧情、界面文本和字体更新。

汉化使用 [Chaldea](https://github.com/chaldea-center/chaldea-data) 的从者、技能、宝具、礼装、物品、任务和活动等映射。剧情使用国服文本，按日服脚本对齐对白、选项和姓名。

## 更新

模块每 24 小时检查翻译和 APK 更新。剧情资源从本仓库的 [resources Release](https://github.com/jhq223/cleat-fgo/releases/tag/resources) 下载，APK 从 [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build/releases/latest) 下载。

APK 下载完成后，在 Android 下载通知中打开安装包。翻译更新完成后重启游戏。

## 导入 Chaldea

Android 10 及以上版本登录成功后，响应文件保存到：

```text
/storage/emulated/0/Download/cleat-fgo/chaldea-login.json
```

在 Chaldea 中选择该文件导入从者和素材数据。详见 [Chaldea 导入指南](https://docs.chaldea.center/zh/guide/import_https/)。每次成功登录会更新这个文件。

## 配置

配置位于 `Android/data/com.aniplex.fategrandorder/files/Mod/config.json`，首次启动时生成。修改后重启游戏。

| 配置项 | 默认值 | 用途 |
|---|---|---|
| `auto_update` | `true` | 自动更新翻译映射和资源包 |
| `update_interval_hours` | `24` | 更新检查间隔，范围 1～168 小时 |
| `resource_manifest_url` | 本仓库 resources Release 的 `manifest.json` | 剧情资源地址；设为 `null` 可关闭 |
| `github_mirror` | `https://gh-proxy.com` | GitHub 下载镜像；设为 `null` 可直连 |
| `record_missing` | `true` | 将待翻译文本记录到 `missing-text.jsonl` |
| `export_login_response` | `true` | 保存 Chaldea 导入文件 |
| `check_apk_updates` | `true` | 检查 APK 更新 |
| `download_apk_updates` | `true` | 自动下载新 APK |

## 补充翻译

将自定义翻译放在 `Mod/overrides/`。例如创建 `ui.json`：

```json
{
  "ゲーム開始": "开始游戏",
  "閉じる": "关闭"
}
```

键为游戏原文，值为译文。保留原文中的 `{0}`、`%s` 等格式参数。

| 文件 | 内容 |
|---|---|
| `ui.json` | 原文到译文的 JSON 映射 |
| `*.script` | 剧情包 |
| `LocalizationJpn.txt` | 界面本地化文本 |
| `Font` | 包含 `FGO-Main-Font-Mod` 的 Unity AssetBundle |
| Chaldea 映射 JSON | 对应类别的翻译数据 |

加载优先级为 `overrides/` → `Mod/` 根目录 → 在线资源。移走本地同名文件即可使用在线版本。

剧情包的制作和发布见 [cleat-fgo-helper](https://github.com/jhq223/cleat-fgo-helper)。

## 构建

将 cleat 与此仓库放在同一目录下，按 [cleat 构建文档](https://github.com/jhq223/cleat/blob/main/docs/zh/BUILD.md)准备 Rust 和 Android NDK，在本仓库执行：

```sh
cargo ndk -t arm64-v8a build --release --locked
```

输出为 `target/aarch64-linux-android/release/libcleat_fgo.so`。APK 打包流程见 [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build)。