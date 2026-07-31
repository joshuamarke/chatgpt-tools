# ChatGPT Tools

轻量、跨平台的 **ChatGPT / Codex 桌面端工具箱**。

- **产品名**：ChatGPT Tools  
- **UI 框架**：Tauri 2  
- **功能域**：皮肤引擎 · 会话管理 · 第三方api渠道切换（后续更多本机工具）  
- **界面**：`src/` 侧栏多视图；皮肤卡片 + 会话列表  

> 非 OpenAI 官方产品。不修改官方 `app.asar` / 安装包签名。皮肤通过本机 `127.0.0.1` CDP 注入；会话管理读写本机 Codex 数据目录。皮肤引擎参考 **[Codex Dream Skin](https://github.com/Fei-Away/Codex-Dream-Skin)** 项目实现，感谢原作者的方案。

*![](https://jsd.itbaihui.com/gh/useritotoo/htmcssimg/2a7e74f692ca384d95cbf2012d7a3071.jpg)*

---

## 功能一览

| 功能 | 说明 |
|------|------|
| **会话管理** | 预览、Markdown 导出、删除可撤销、历史 provider 修复、session_index 清理 |
| **供应商管理** | 支持开启本地路由，多供应商切换，支持还原官方默认登录，支持codex解锁第三方模型列表 |
| 多皮肤浏览 | 内置多套皮肤，CDP注入在不切换深色/浅色模式时可以热切皮肤不用重启。 |
| 一键应用 / 还原 | 自动处理调试端口与可选重启；慢启动 lifecycle 探测 |
| 大图立绘 | 允许高质量原图；shell 先成功，立绘异步贴入 |
| 暂停 / 继续 | 不杀客户端的卸皮 / 恢复（CLI：`pause` / `resume`） |
| 导入 / 导出 | `.skin` / `.zip` + inspect 风险扫描 |
| 自定义皮肤 | 基于目标皮肤模板；自适应构图 + 自定义颜色 / 壁纸（≤16 MB） |
| 共享渲染内核 | 新增皮肤只需 CSS + `plugin.json`（无 inject 脚本），不必改 core |
| 宿主选择器契约 | `engine/runtime/selectors.json` + `npm run doctor:selectors`；模板 `skins/_template` |
| 纯 Rust 热路径 | 日常 apply/status/restore 可不依赖系统 Node；页内 Operation UI |
| 指定客户端 | 手动选择 ChatGPT / Codex 路径 |
| 非侵入 | 不改官方安装目录 |

## 环境要求

| 组件 | 版本 | 用途 |
|------|------|------|
| **Node.js** | ≥ 18 | 注入引擎（`运行`可不依赖node）与开发依赖 |
| **Rust / Cargo** | stable | 编译 Tauri 2 |
| **Tauri 系统依赖** | 见官方文档 | Windows：WebView2（Win10/11 通常已自带） |
| **ChatGPT / Codex 桌面版** | 已安装 | 换肤目标 |

---

## 快速开始

```bash
cd chatgpt-tools   # 或你的克隆路径
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
状态目录：`%LOCALAPPDATA%\ChatGPTTools\`。

---

## 打包

```bash
npm run build
```

产物位于 `src-tauri/target/release/bundle/`（NSIS / MSI / 可执行文件等，视平台而定）。

打包前请确认：

- `engine/` 与**内置皮肤**已列入 `bundle.resources`（见下）
- `bundle.active` 为 `true`（安装包 / DMG 才会生成）

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
├── scripts/                     # 构建 / 发版 / 契约与冒烟（无探针杂项）
├── package.json
└── README.md
```

## 皮肤引擎 

引擎内核与 Tauri 多皮肤管理：参考 **[Codex Dream Skin](https://github.com/Fei-Away/Codex-Dream-Skin)** 项目实现感谢原作者的方案。

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
npm run test:engine          # injector 自检（无需宿主）
npm run verify:engine        # CLI 冒烟
npm run doctor:selectors     # 宿主选择器契约
npm run check:gui            # GUI 静态回归
npm run engine -- check-payload --skin-id qingkong
npm run engine -- pause
npm run engine -- resume
npm run engine -- verify --skin-id qingkong
```

新建皮肤见 [docs/development/create-skin.md](docs/development/create-skin.md)。

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

## License

MIT（内置同人立绘仅供学习美化，请勿商用未授权素材）。
