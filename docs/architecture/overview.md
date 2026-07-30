# 架构总览（ChatGPT Tools）

## 1. 目标

**ChatGPT Tools** 用 **Tauri 2** 做轻量壳，面向 ChatGPT / Codex 桌面端提供**多功能本机工具箱**：

- 更小的安装包与运行时内存（相对 Electron 壳）  
- GUI 与各功能域解耦；**皮肤不是唯一主线**  
- **皮肤**：多皮肤目录、导入导出、CDP 注入、共享渲染内核  
- **会话**：本机 SQLite 会话浏览 / 清理（带备份）  
- 后续功能按 [features.md](./features.md) 的目录约定扩展  

皮肤引擎内核（仍属 Skins 域）：**载荷控制、CDP 身份、共享渲染、稳定生命周期、慢启动探测、大图立绘**。

## 2. 分层

| 层 | 路径 | 职责 |
|----|------|------|
| Presentation | `src/` | 侧栏多视图、模态框、Toast；不直接碰文件系统 |
| Feature UI | `src/features/<domain>/` | 各功能前端（如 `sessions/`） |
| IPC API | `skin-api.js` · `sessionAPI` · `commands` / `sessions/commands` | 分命名空间的 invoke 表面 |
| Host | `src-tauri/` | 窗口、对话框、权限、资源路径 |
| Skins engine | `engine/` + `src-tauri/cdp/` | 皮肤清单、CDP 注入、导入导出 |
| Sessions domain | `src-tauri/sessions/` | Codex home 发现、SQLite 列表/删除、备份 |
| Host probe | `engine/host-probe.js` | 进程 / CDP 端口 / app:// 三信号生命周期 |
| Shared runtime | `engine/runtime/renderer-core.js` | 多皮肤共用：ensure/cleanup/shell-guard/自适应 |
| Payload | `engine/payload.mjs` + `image-metadata.mjs` | 指纹缓存、立绘安全、组装注入脚本 |
| Assets | `skins/` | 内置皮肤包（**仅** CSS + art + plugin.json，无 inject） |

### 数据流（应用皮肤）

```text
用户点击「使用此皮肤」
  → skinAPI.apply(id, { restart })
  → commands::apply
  → node engine/cli.mjs apply --skin-id … --restart …
  → manager.applySkin（async 互斥锁）
       · materializeSkin
       · probeHostLifecycle + ensureDebugPort（慢启动两阶段等待）
       · 读 CDP Browser 身份
       · purge-all / 停旧 injector（同皮肤热路径可跳过）
       · startInjector watch --browser-id --pause-file
       · soft once：shell 成功即返回；art 异步/大图超时 → artPending
  → JSON 结果返回 UI（shellOk / artOk / artPending / lifecycle）
```

## 3. 引擎 v2.2 要点

| 能力 | 说明 |
|------|------|
| 共享 renderer-core | 增删皮肤只改 `skins/<id>`，不改 core |
| 载荷管线 | 16MB/50MP/16384px 硬限；magic MIME；fingerprint 缓存 |
| **大尺寸原图** | 立绘可多 MB 以保壁纸质量；shell 先成功；art 按体积拉长 CDP 超时；推荐另备 `screenshot` 缩略图 |
| CDP 身份 | loopback only、命令超时、畸形帧关闭、Browser 锚、端口复用拒连 |
| soft / hard verify | soft：root+style；hard：主壳+可选侧栏 |
| **宿主生命周期** | `offline` / `starting` / `ready`（进程 ∪ 端口 ∪ app://） |
| 暂停 | `paused.flag` + remove；watch 保持 |
| 自适应 | `appearance` + `art.focus/safeArea/taskMode`；Canvas 分析可缓存 |
| watch | Node 回退：`fs.watch` + 轮询；**主路径**用进程内 keep（退避探测，非 1.8s 硬刷） |
| 宿主稳态 | `renderer-core`：`warm`→`steady`→`hidden`；成功后无全树 MutationObserver |

## 4. 稳定契约（勿随意破坏）

### 4.1 前端 `window.skinAPI`

- `status` / `detect` / `apply` / `restore`
- `exportSkin` / `importSkin` / `deleteSkin`
- `designWallpaper` / `chooseWallpaper`
- `chooseApp` / `clearAppPath`
- `openPath` / `openExternal` / `revealExport`

扩展：`enginePaths` / `engineVersion` / `hostStatus({ force })`；  
status / hostStatus 含 `paused` / `protocol` / `lifecycle` / `lifecycleRaw` / `confidence` / `canHotApply` / `needsRestartForInject` / `shellOk` / `artOk`。

皮肤来源：`bundled` \| `user`（本机导入）等。

**GUI 轮询**：pill 用轻量 `hostStatus`（TTL 缓存 + 滞回）；全量 `status` 仅在启动 / 换肤 / 导入后刷新皮肤列表。

### 4.2 引擎 CLI

见 [engine-cli.md](./engine-cli.md)。`protocol: 2`，`engineVersion: 2.2.x`。

### 4.3 状态目录

- 默认：`ChatGPTTools`（`CODEX_SKIN_STATE_NAME` 可覆盖）  
- 资源根：`CODEX_SKIN_ROOT`  

## 5. 安全边界

- 注入仅限 `127.0.0.1` CDP  
- 不修改官方安装包  
- 导入包静态扫描（网络/eval/fs）+ 立绘体积提示  
- 渲染进程无 Node；敏感操作在 engine / Rust  
- stop injector 校验命令行身份，避免误杀  

## 6. 与 Codex Dream Skin 的关系

| 项 | Dream Skin | 本项目 |
|----|------------|--------|
| GUI | 托盘/菜单栏 | Tauri 多皮肤管理 |
| 主题模型 | 单活动主题库 | 多皮肤 + .skin |
| 注入硬化 | 深度 | **已对齐核心**（身份/载荷/shell-guard） |
| 渲染 | 平台分叉 CSS | **统一 core + 每皮肤 plugin** |
| 宿主选择器 | 每端 selectors.json | **统一** `engine/runtime/selectors.json` + doctor |
| Operation UI | 页内进度 | **右下角 toast**（renderer-core + native inject；keep 静默） |
| 免系统 Node | 安装器内嵌/官方 Node | **Rust CDP 主路径** `nodeRequired: false` |
| macOS 启动 | launchd + open | **`open -n -a` + 调试 flag**（统一引擎） |
| Win Store 更新 | AUMID + 包身份 schema | **state schema 3 + 每次 re-resolve** |

## 7. 模块职责（engine）

| 文件 | 职责 |
|------|------|
| `cli.mjs` | 对外 JSON 协议，Tauri 唯一引擎入口 |
| `version.js` | 单一版本 / protocol 源 |
| `host-probe.js` | 宿主进程与 CDP 生命周期探测 |
| `manager.js` | 皮肤清单、互斥锁、apply/restore/pause、导入导出 |
| `injector.mjs` | CDP 会话、watch/once/verify/remove、soft 分层 |
| `payload.mjs` | `buildStagedPayload` / `buildDeltaShellPayload` / 缓存 |
| `image-metadata.mjs` | PNG/JPEG/WebP 尺寸与炸弹拒绝 |
| `runtime/renderer-core.js` | host 常驻、`applySkin` delta、`applyArt`、lifeMode 稳态、Operation UI、registry |
| `runtime/selectors.json` | 宿主 DOM 契约（皮肤作者找锚点用） |
| `runtime/operation-ui.js` | 页内操作反馈（亦可由 core 内联） |
| `purge-all.mjs` | 注册表 + 遗留 markers 全量清理 |

**扩展皮肤**：只加 `skins/<id>/`，**不要**改 core；文案/IP 只放 `plugin.json`。

## 8. 路线图（已落地 / 后续）

| 项 | 状态 |
|----|------|
| 载荷 + CDP 身份 + 共享 core | 完成 |
| 两阶段 shell→art | 完成 |
| 慢启动 lifecycle 探测 | **1.1.2 完成** |
| 大图原图 + 缩放超时 + 分块解码 | **1.1.2 完成** |
| watch fs.watch + 降频轮询 | **1.1.2 完成** |
| 长驻 injector 热换（control 文件 switch） | **1.1.3 完成** |
| slim core 常驻 + deltaShell | **1.1.3 完成** |
| GUI pause/resume | CLI 已有；GUI 可选 |
| 纯 Rust CDP 热路径（去系统 Node spawn） | **1.1.3 阶段 1–2**：host ready 时 apply；status/detect/version/paths/resolve-asset/settings/delete/restore 走 `src-tauri/src/cdp/` |
| 冷启动 / restart 原生化 | **1.1.3.1**：`host` 三信号 lifecycle + `launch::ensure_debug_port`；apply/restore 互斥 |
| 进程内 re-inject + 导入导出 | **1.1.3.2**：`keep` 后台线程刷新保持；`package` 原生 `.skin` export/import/inspect；design-wallpaper 仍可回退 Node |
| 稳态宿主 + keep 退避 | **1.1.4**：页内 warm→steady 生命周期（无全树 MO）；keep 指数退避 + 可选 art 恢复；冷路径条件化 repair |
| selectors 契约 + 模板 + Operation UI | **1.1.5**：`selectors.json` / `_template` / 页内操作反馈 |
| macOS 启动链 + Win Store schema3 | **1.1.5**：`open -a` 调试启动；包身份写入 state；stale 自动 re-resolve |
| delta 命中率 | **1.1.5**：同 revision CSS 短路 `deltaHit`；inject 上报 `shellMode`/`deltaHits` |
| GUI / Store UX | **1.1.6**：status 暴露 `storePackage`/`shellMode`；多包确认框；apply toast 带热切换提示 |
| macOS 可选 launchd | **1.1.7**：`scripts/macos/install-debug-launch-agent.sh`（默认仍走 open -a） |
| **会话管理（列表/删除/备份）** | **已落地**：侧栏入口 + `src-tauri/sessions` + `sessionAPI`；见 [../features/sessions.md](../features/sessions.md) |
| 会话导出 / provider 修复 / index 清理 | **Phase 2 已落地**：Markdown 导出、GUI 撤销、provider 历史修复、`session_index` 孤儿清理 |
| 更多工具功能域 | 按 [features.md](./features.md) 扩展 |
