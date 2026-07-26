# 模块地图（改哪里）

面向：皮肤作者、引擎维护者、以及**新功能域**开发者。  
原则：个性化只动 `skins/<id>/`；宿主锚点与框架能力有固定入口；**新功能**落在 `src-tauri/<domain>/` + `src/features/<domain>/`，不要塞进皮肤引擎。

功能域总览：[../architecture/features.md](../architecture/features.md)。

## 0. 环境概览

| 目标 | 路径 |
|------|------|
| 本机安装探测（桌面 / CLI / Grok） | `src-tauri/src/env/mod.rs` |
| Tauri 命令 | `env_check`（`lib.rs` 注册） |
| 前端 API | `src/skin-api.js` → `envCheck({ force })` |
| 侧栏「概览」+ 内容区 | `src/index.html#overviewView` · `src/app.js`（`setMainView("overview")` · `loadEnvironment`） |
| 样式 | `src/styles.css`（`.overview-view` · `.ov-card` …） |
| 说明 | 皮肤仅桌面端；**Codex CLI 不支持皮肤** |

## 0b. 会话管理

| 目标 | 路径 |
|------|------|
| Codex 列表 / 删除 / 撤销 | `src-tauri/src/sessions/storage.rs` · `backup.rs` |
| Codex Markdown 导出 | `sessions/markdown.rs` |
| Grok 扫描 / 删除 / 导出 | `sessions/grok.rs`（`~/.grok/sessions`） |
| Provider 历史修复 · index 清理 | `sessions/provider_sync.rs`（仅 Codex Tab） |
| DB 发现 · Codex home | `sessions/discovery.rs` · `sessions/home.rs` |
| 删除备份路径 | `sessions/paths.rs`（应用 state）· provider 备份在 `{CODEX_HOME}/backups_state/provider-sync/` |
| Tauri 命令 | `sessions/commands.rs`（注册于 `lib.rs`；含 `list_grok_sessions` 等） |
| 前端 API | `src/features/sessions/sessions-api.js` |
| 列表 UI · 来源 Tab | `src/features/sessions/sessions-view.js` · `index.html#sessionsView` · `#sessTabs` |
| 侧栏入口 / 视图切换 | `src/app.js`（`setMainView("sessions")`） |
| 样式 | `src/styles.css`（`.sessions-view` · `.sessions-tabs` · `.sess-undo-bar` …） |
| 说明文档 | `docs/features/sessions.md` |

## 0c. 供应商管理

| 目标 | 路径 |
|------|------|
| 档案存储（JSON） | `src-tauri/src/providers/store.rs` → `%LOCALAPPDATA%\ChatGPTTools\providers.json` |
| Codex live 写入 | `providers/codex.rs`（双写 auth.json + config.toml · wire_api · backfill） |
| Grok live 写入 | `providers/grok.rs`（config.toml · 校验 · 保留 MCP · backfill） |
| 渠道预设 | `providers/presets.rs` |
| 领域类型 | `providers/models.rs`（含 `LiveStatus` · `activate` · `wireApi`） |
| Tauri 命令 | `providers/commands.rs`：list/get/add/update/delete/switch/import/paths/**presets**/ **reapply** |
| 前端 API | `src/features/providers/providers-api.js` → `window.providerAPI` |
| 列表 UI · 应用 Tab | `providers-view.js` · `index.html#providersView` · `#provTabs` · live 条 · 保存并启用 |
| 侧栏入口 / 视图切换 | `src/app.js`（`setMainView("providers")`） |
| 样式 | `src/styles.css`（`.providers-view` · `.prov-live-bar` · `.prov-card` …） |
| 说明文档 | `docs/features/providers.md` |

## 1. 皮肤作者（自由度高、边界清晰）

| 目标 | 路径 |
|------|------|
| 从零新建 | 复制 `skins/_template/` → `skins/<id>/` |
| 显示名 / 构图 / desktop chrome | `skins/<id>/skin.json` |
| 色板、布局、面板、建议卡 | `skins/<id>/assets/*.css` |
| 品牌文案 / 装饰 HTML | `skins/<id>/assets/plugin.json` |
| 壁纸 / 卡片缩略图 | `assets/art.*` · `assets/screenshot.*` |
| **宿主元素叫什么** | **`engine/runtime/selectors.json`** |
| 契约约定（建议遵守） | `docs/development/skin-contract.md` |
| 步骤说明 | `docs/development/create-skin.md` |

**不要**：复制 `renderer-core.js` / `immersive-skin.css` 进皮肤包；写 per-skin inject。

## 2. 引擎维护（全局一次，全皮肤受益）

| 目标 | 路径 |
|------|------|
| shell-guard / 热换 delta / Operation UI | `engine/runtime/renderer-core.js` |
| 全窗壁纸 / 原生可读默认 CSS | `engine/runtime/immersive-skin.css` |
| 宿主选择器契约 + doctor | `engine/runtime/selectors.json` · `npm run doctor:selectors` |
| 载荷 shell/art/delta 组装 | `engine/payload.mjs` · `src-tauri/src/cdp/payload.rs` |
| 纯 Rust 注入 / soft verify | `src-tauri/src/cdp/inject.rs` |
| 刷新保持（免 Node watch） | `src-tauri/src/cdp/keep.rs` |
| 启动宿主（macOS open / Win Store） | `src-tauri/src/cdp/launch.rs` |
| macOS 可选 launchd | `scripts/macos/install-debug-launch-agent.sh` · `docs/development/macos-launch.md` |
| apply 状态 schema / Store 身份 | `src-tauri/src/cdp/native.rs`（status 含 `storePackage` / `shellMode`） |
| GUI 换肤反馈 | `src/app.js`（`formatApplySuccessToast` / multi-package 确认） |
| 生命周期三信号 | `src-tauri/src/cdp/host.rs` · `engine/host-probe.js` |
| Node 回退 CLI | `engine/cli.mjs` · `engine/manager.js` · `engine/injector.mjs` |

## 3. 用户体验主路径（免系统 Node）

```text
GUI apply
  → Rust native apply (CODEX_SKIN_NATIVE=1 默认)
  → launch.ensure_debug_port（冷启动 / 重启）
  → inject_once_with_opts（delta shell → soft verify；OpUI 右下角 toast）
  → shell_ready 即返回（feedback=host|gui；无热路径 PowerShell）
  → art 后台线程（cap CDP 超时；失败不阻断 shell 成功）
  → keep 静默 re-inject（无 OpUI、不附 art）
  → state.json schema 3（含 Store 包身份缓存字段）
```

强制 Node：`CODEX_SKIN_FORCE_NODE=1`。  
关闭原生：`CODEX_SKIN_NATIVE=0`。

## 4. 校验命令

```bash
npm run doctor:selectors
npm run test:engine
npm run check:payloads
node engine/cli.mjs check-payload --skin-id dream
```
