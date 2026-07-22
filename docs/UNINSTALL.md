# 卸载 ChatGPT Tools

## 1. 应用内

先点 **「完全还原」**，恢复 ChatGPT 官方界面与 `config.toml` 中的相关主题改动。

## 2. 删除程序

- 安装包：用系统卸载程序删除 **ChatGPT Tools**
- 便携/开发：删除安装或克隆目录即可

## 3. 状态目录（可选清理）

| 平台 | 路径 |
|------|------|
| Windows | `%LOCALAPPDATA%\ChatGPTTools\` |
| macOS | `~/Library/Application Support/ChatGPTTools/` |

内含：`state.json`、`settings.json`、用户导入皮肤、`runtime-skins`、注入日志。

## 4. 勿删除

- ChatGPT / Codex 官方安装目录  
- 除非你明确要重置 Codex：一般保留 `~/.codex/` 中的 auth 与项目配置  

## 5. 与旧版并存时

当前状态目录为 `ChatGPTTools`。历史目录可能是：

- `%LOCALAPPDATA%\CodexSkin\`
- `%LOCALAPPDATA%\CodexSkinManager\`

新版启动时会尽量合并到 `ChatGPTTools`。卸载时若仍存在旧目录，可一并删除。
