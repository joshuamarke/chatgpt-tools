# ChatGPT Tools（Tauri 2）

轻量、跨平台的 **ChatGPT / Codex 桌面端换肤工具箱**。

- **产品名**：ChatGPT Tools  
- **UI 壳**：Tauri 2（系统 WebView，体积与内存远低于 Electron）  
- **业务引擎**：`engine/`（Node.js CDP 注入，**v2.2 共享 runtime**）  
- **界面**：`src/` 皮肤卡片管理  

> 非 OpenAI 官方产品。不修改官方 `app.asar` / 安装包签名；仅通过本机 `127.0.0.1` CDP 注入样式与装饰层。

---

## 功能一览

| 功能 | 说明 |
|------|------|
| 多皮肤浏览 | 内置多套皮肤卡片预览 |
| 一键应用 / 还原 | 自动处理调试端口与可选重启；慢启动 lifecycle 探测 |
| 大图立绘 | 允许高质量原图；shell 先成功，立绘异步贴入 |
| 暂停 / 继续 | 不杀 Codex 的卸皮 / 恢复（CLI：`pause` / `resume`） |
| 导入 / 导出 | `.cgskin` / `.zip` + inspect 风险扫描 |
| 设计壁纸 | 自适应构图字段 + 自定义颜色 |
| 共享渲染内核 | 新增皮肤只需 CSS + `plugin.json`（无 inject 脚本），不必改 core |
| 指定客户端 | 手动选择 ChatGPT / Codex 路径 |
| 非侵入 | 不改官方安装目录 |

---

## 环境要求

| 组件 | 版本 | 用途 |
|------|------|------|
| **Node.js** | ≥ 18 | 注入引擎（`engine/cli.mjs`）与开发依赖 |
| **Rust / Cargo** | stable | 编译 Tauri 壳 |
| **Tauri 系统依赖** | 见官方文档 | Windows：WebView2（Win10/11 通常已自带） |
| **ChatGPT / Codex 桌面版** | 已安装 | 换肤目标 |

---

## 快速开始

```bash
cd E:\临时项目\chatgpt-tools   # 或你的克隆路径
npm install
npm run dev
```

开发模式会：

1. 启动 Tauri 窗口加载 `src/index.html`  
2. 通过 Rust `invoke` 调用 `node engine/cli.mjs …`  
3. 引擎读写状态目录并（按需）启动 CDP 注入守护进程  

仅调试引擎（无 GUI）：

```bash
npm run engine -- status
npm run engine -- list-skins
npm run engine -- apply --skin-id dream --restart true
```

新建 / 改名皮肤：见 [docs/development/create-skin.md](docs/development/create-skin.md)。  
状态目录：`%LOCALAPPDATA%\ChatGPTTools\`（旧版 `CodexSkin` / `CodexSkinManager` 会自动合并）。

---

## 打包

```bash
npm run build
```

产物位于 `src-tauri/target/release/bundle/`（NSIS / MSI / 可执行文件等，视平台而定）。

打包前请确认：

- `engine/`、`skins/` 已列入 `src-tauri/tauri.conf.json` → `bundle.resources`
- 目标机器仍安装 **Node.js**（当前阶段注入未内嵌 Node）

---

## 仓库结构

```text
chatgpt-tools/
├── src/                         # 前端 UI
│   ├── index.html
│   ├── styles.css
│   ├── app.js
│   └── skin-api.js              # window.skinAPI → Tauri invoke
├── engine/                      # 换肤引擎 v2.2（Node）
│   ├── cli.mjs                  # 稳定 JSON CLI（protocol 2）
│   ├── version.js               # 引擎版本单一源
│   ├── host-probe.js            # 进程/端口/app:// 生命周期
│   ├── manager.js               # 互斥 / apply / 导入导出 / 壁纸
│   ├── injector.mjs             # CDP 会话 + soft/hard verify + watch
│   ├── payload.mjs              # 指纹缓存 + 共享 runtime 组装
│   ├── image-metadata.mjs       # 立绘硬限 / 像素炸弹防护
│   ├── purge-all.mjs            # 多 markers + 注册表清理
│   ├── runtime/renderer-core.js # 多皮肤共享渲染（增删皮肤不改此文件）
│   └── tests/                   # node:test 自检
├── skins/<id>/                  # 内置皮肤：CSS + art + plugin.json
├── src-tauri/                   # Tauri 2 / Rust
├── docs/
├── scripts/                     # verify-engine / migrate-plugins
├── package.json
└── README.md
```

## 引擎 v2.2（载荷与多皮肤）

对齐参考项目的 **engine 内核**，产品形态仍是 Tauri 多皮肤管理：

| 能力 | 说明 |
|------|------|
| 共享渲染 | `renderer-core.js`；皮肤只提供 CSS + `plugin.json`（文案/IP） |
| 载荷管线 | magic MIME、≤16MB/50MP、fingerprint 缓存、`check-payload` |
| 大图原图 | 允许多 MB 壁纸；shell 先成功；art 超时按体积缩放 |
| 宿主探测 | `offline` / `starting` / `ready`（进程 ∪ CDP ∪ app://） |
| CDP 身份 | loopback、Browser 锚、停 injector 前命令行身份校验 |
| soft / hard | apply 默认 soft once；`verify` 为 hard（侧栏可选） |
| 生命周期 | async 互斥锁、pause/resume、`purge-all`、同皮肤热路径 |
| 自适应 | `appearance: auto` 跟随宿主；`art.focus/safeArea/taskMode` |

常用命令：

```bash
npm run test:engine
npm run verify:engine
npm run engine -- check-payload --skin-id dream
npm run engine -- pause
npm run engine -- resume
npm run engine -- verify --skin-id dream
```

新建皮肤：**不必改** `renderer-core.js`，见 [docs/development/create-skin.md](docs/development/create-skin.md)。

---

## 状态与卸载

| 平台 | 状态目录 |
|------|----------|
| Windows | `%LOCALAPPDATA%\ChatGPTTools\` |
| macOS | `~/Library/Application Support/ChatGPTTools/` |

卸载建议：

1. 应用内「完全还原」  
2. 删除应用安装目录  
3. 删除上述状态目录（含用户导入皮肤、日志）  
4. 不触碰 ChatGPT / Codex 官方安装路径  

详见 [docs/UNINSTALL.md](./docs/UNINSTALL.md)。

---

## 文档索引

| 文档 | 内容 |
|------|------|
| [docs/architecture/overview.md](./docs/architecture/overview.md) | 架构、模块边界、扩展点 |
| [docs/architecture/engine-cli.md](./docs/architecture/engine-cli.md) | 引擎 CLI 协议 |
| [docs/development/setup.md](./docs/development/setup.md) | 开发环境与排错 |
| [docs/development/create-skin.md](./docs/development/create-skin.md) | 新建皮肤 |
| [docs/development/migration.md](./docs/development/migration.md) | 自 Electron 迁移说明 |
| [docs/UNINSTALL.md](./docs/UNINSTALL.md) | 干净卸载 |

---

## License

MIT（内置同人立绘仅供学习美化，请勿商用未授权素材）。
