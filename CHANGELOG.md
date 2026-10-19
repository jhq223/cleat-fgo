# 修改历史

## 0.2.3

- 登录响应通过 Android 下载文件接口保存到 `Download/cleat-fgo/chaldea-login.json`，可由文件管理器读取并在 Chaldea 中选择导入。

## 0.2.2

- 剧情资源和 APK 支持分段续传，下载中断后继续已有进度。
- 修复 CI 中 Android SDK 工具包的安装配置。

## 0.2.0

- 使用 cleat 0.3、内部 bridge 与 Frida Gum，适配当前 Unity 6 和 ARM64 模拟器转译环境。
- 扩大 Chaldea 翻译覆盖，支持嵌套映射，更新冠位和 Beast 职介名称。
- 后台更新映射及 resources Release 资源包，校验后原子切换；支持 GitHub 镜像、失败回退及本地覆盖。
- 增加 build 仓库 APK 检查、版本比较、校验下载及 Android 下载通知。
- 默认启用 60 FPS。
- 增加登录成功响应本地导出，支持 JSON 与 Base64 包装，供 Chaldea 导入。
- 主数据按帧预算处理，使用有 GC 根的集合快照；异步载入字体并刷新已有标签。
- 移除手动密钥导出 Hook，资源流程由 helper 自动处理。
