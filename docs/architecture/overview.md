# 架构总览（ChatGPT Tools）

## 1. 目标

**ChatGPT Tools** 用 **Tauri 2** 做轻量壳，用 **Node 引擎** 对 ChatGPT / Codex 桌面端做本机 CDP 换肤：

- 更小的安装包与运行时内存（相对 Electron 壳）  
- GUI 与注入引擎解耦  
- **多皮肤**目录、导入导出、设计壁纸  
- 引擎内核：**载荷控制、CDP 身份、共享渲染、稳定生命周期、慢启动探测、大图立绘**

## 2. 分层

| 层 | 路径 | 职责 |
|----|------|------|
| Presentation | `src/` | 皮肤卡片、模态框、Toast；不直接碰文件系统 |
| IPC API | `src/skin-api.js` + `src-tauri/src/commands.rs` | 稳定的 `skinAPI` / `invoke` 表面 |
| Host | `src-tauri/` | 窗口、对话框、权限、资源路径、进程桥接 |
| Domain Engine | `engine/` | 皮肤清单、导入导出、启动 Codex、CDP 注入、config.toml |
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
| watch | `fs.watch` 驱动主题重建；轮询降频；early shell + 大图 art |

## 4. 稳定契约（勿随意破坏）

### 4.1 前端 `window.skinAPI`

- `status` / `detect` / `apply` / `restore`
- `exportSkin` / `importSkin` / `deleteSkin`
- `designWallpaper` / `chooseWallpaper`
- `chooseApp` / `clearAppPath`
- `openPath` / `openExternal` / `revealExport`

扩展：`enginePaths` / `engineVersion` / `hostStatus({ force })`；  
status / hostStatus 含 `paused` / `protocol` / `lifecycle` / `lifecycleRaw` / `confidence` / `canHotApply` / `needsRestartForInject` / `shellOk` / `artOk`。

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
| 主题模型 | 单活动主题库 | 多皮肤 + .cgskin |
| 注入硬化 | 深度 | **已对齐核心**（身份/载荷/shell-guard） |
| 渲染 | 平台分叉 CSS | **统一 core + 每皮肤 plugin** |

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
| `runtime/renderer-core.js` | host 常驻、`applySkin` delta、`applyArt`、registry |
| `purge-all.mjs` | 注册表 + 遗留 markers 全量清理 |

**扩展皮肤**：只加 `skins/<id>/`，**不要**改 core；文案/IP 只放 `plugin.json`。

## 8. 路线图（已落地 / 后续）

| 项 | 状态 |
|----|------|
| 载荷 + CDP 身份 + 共享 core | 完成 |
| 两阶段 shell→art | 完成 |
| 慢启动 lifecycle 探测 | **2.2 完成** |
| 大图原图 + 缩放超时 + 分块解码 | **2.2 完成** |
| watch fs.watch + 降频轮询 | **2.2 完成** |
| 长驻 injector 热换（control 文件 switch） | **2.3 完成** |
| slim core 常驻 + deltaShell | **2.3 完成** |
| GUI pause/resume | CLI 已有；GUI 可选 |
| 纯 Rust CDP 热路径（去系统 Node spawn） | **2.3 阶段 1–2**：host ready 时 apply；status/detect/version/paths/resolve-asset/settings/delete/restore 走 `src-tauri/src/cdp/` |
| 冷启动 / restart 原生化 | **2.3.1**：`host` 三信号 lifecycle + `launch::ensure_debug_port`；apply/restore 互斥 |
| 进程内 re-inject + 导入导出 | **2.3.2**：`keep` 后台线程刷新保持；`package` 原生 `.cgskin` export/import/inspect；design-wallpaper 仍可回退 Node |
