# cleat-fgo

Fate/Grand Order（日服）翻译模组 — 基于 [cleat](https://github.com/jhq223/cleat)。

## 功能

### Master Data 翻译

通过 IL2CPP hook 在运行时补丁 21 张游戏数据表：

| 类别 | 示例 |
|---|---|
| 从者 | 名称（含各阶段覆写） |
| 技能、宝具 | 名称、描述、注音、类型 |
| 礼装、指令纹章 | 名称、描述 |
| 灵衣、魔术礼装 | 名称、详细信息 |
| 物品、商店 | 名称 |
| 关卡、地图、活动 | 名称 |
| Buff、任务 | 名称、描述 |

处理 FGO 特有的 `ObscuredString` 加密，并在匹配翻译前剥离富文本标记。

### Script Bundle 替换

解密 `.script` bundle 文件（TextBundle 格式：Rijndael-256-CBC → zlib → UTF-16LE），
替换游戏内剧情/对话文本。

### 职阶名翻译

将职阶 ID 翻译为英文（`Saber`、`Archer`、`Berserker` …）。

### 自定义字体

从 Mod 文件夹加载 Unity Font 资源包，替换所有 `UILabel` 的默认字体。

### 自定义本地化

将 `LocalizationJpn.txt` 注入 `LocalizationManager.SetTextData`。

## Mod 文件夹

放入 `/storage/emulated/0/Android/data/com.aniplex.fategrandorder/files/Mod/`：

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
├── *.script              # TextBundle 文件
├── LocalizationJpn.txt   # UTF-16LE 编码
└── Font                  # Unity Font 资源包
```

翻译 JSON 使用 [chaldea-data] 格式，可从
[chaldea-data/mappings](https://github.com/chaldea-center/chaldea-data/tree/main/mappings)
下载：

```json
{
  "アルトリア・ペンドラゴン": { "CN": "阿尔托莉雅·潘德拉贡" },
  "エミヤ": { "CN": "英灵卫宫" }
}
```

Key 在匹配前会做全角→半角标准化。

[chaldea-data]: https://github.com/chaldea-center/chaldea-data

## 编译

**依赖**

- Rust nightly
- Android NDK r26+
- [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) ≥ 3.0

```bash
cargo ndk -t arm64-v8a -o ./target/jniLibs build --release
# → target/jniLibs/arm64-v8a/libcleat_fgo.so
```

## License

MIT
