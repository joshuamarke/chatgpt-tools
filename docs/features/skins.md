# 皮肤功能

对 ChatGPT / Codex 桌面端做本机 CDP 换肤：多皮肤目录、导入导出、自定义壁纸、暂停/恢复、云目录等。

皮肤是产品中的**成熟功能域**，与 [会话管理](./sessions.md) 等工具并列，而非仓库唯一主线。

## 代码落点

| 区域 | 路径 |
|------|------|
| GUI | `src/app.js` · `src/skin-api.js` · `src/skin-categories.json` |
| Node 引擎 | `engine/` |
| 原生 CDP | `src-tauri/src/cdp/` |
| 云目录 | `src-tauri/src/cloud/` |
| 内置皮肤 | `skins/<id>/` |

IPC：`window.skinAPI`（见 `src/skin-api.js`）。

## 皮肤包

| 项 | 说明 |
|----|------|
| 扩展名 | **`.skin`**（zip）；导入也接受 `.zip` 与遗留 `.cgskin` |
| 结构 | zip 根为 `skin/`（或直接含 `skin.json`） |
| 云 catalog | `package.format` 固定为 `skin` |
| 导出 | GUI「导出」或 CLI `export-skin` → 默认 `*.skin` |
| 导入 | GUI「导入皮肤」或 CLI `import-skin` / `inspect-skin` |

与 CDN 仓打包脚本对齐：`chatgpt-tools-cdn` 的 `npm run pack:skin` 输出 `{id}-{version}.skin`。

## 文档索引

| 文档 | 内容 |
|------|------|
| [../architecture/overview.md](../architecture/overview.md) | 分层与数据流 |
| [../architecture/engine-cli.md](../architecture/engine-cli.md) | CLI 协议 |
| [../development/skin-contract.md](../development/skin-contract.md) | 皮肤契约 |
| [../development/create-skin.md](../development/create-skin.md) | 新建皮肤 |
| [../development/module-map.md](../development/module-map.md) | 改哪里 |
| [../cloud-integration.md](../cloud-integration.md) | 云端集成 |

## 状态目录

`%LOCALAPPDATA%\ChatGPTTools\`（`CODEX_SKIN_STATE_NAME` 可覆盖）：`state.json`、`paused.flag`、用户皮肤等。

注意：这里的「session / skinId」指**上次应用的皮肤会话**，与 Codex 聊天 thread 无关。
