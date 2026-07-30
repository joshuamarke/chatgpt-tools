# ChatGPT Tools（Tauri 2）

轻量、跨平台的 **ChatGPT / Codex 桌面端工具箱**。

- **产品名**：ChatGPT Tools  
- **UI 壳**：Tauri 2（系统 WebView，体积与内存远低于 Electron）  
- **功能域**：皮肤引擎 · 会话管理 ·（后续更多本机工具）  
- **界面**：`src/` 侧栏多视图；皮肤卡片 + 会话列表  

> 非 OpenAI 官方产品。不修改官方 `app.asar` / 安装包签名。皮肤通过本机 `127.0.0.1` CDP 注入；会话管理读写本机 Codex 数据目录。

皮肤是当前最完整的模块之一，但**不是**产品唯一主线。功能域规划见 [docs/architecture/features.md](docs/architecture/features.md)。

---

## 功能一览

| 功能 | 说明 |
|------|------|
| **会话管理** | 侧栏入口：分页浏览、Markdown 导出、删除可撤销、历史 provider 修复、session_index 清理 |
| 多皮肤浏览 | 内置多套皮肤卡片预览 |
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

会话管理说明：[docs/features/sessions.md](docs/features/sessions.md)。  
皮肤说明：[docs/features/skins.md](docs/features/skins.md)。  
供应商 / 本地路由 / 请求日志：[docs/features/providers.md](docs/features/providers.md)。

### 版本号规范（`MAJOR.MINOR.PATCH`，当前 **1.1.12**）

| 位 | 含义 | 何时递增 |
|----|------|----------|
| **MAJOR**（第 1 位） | 大方向 / 产品主线 | 整体方向级变更，例如 `2.1.12` |
| **MINOR**（第 2 位） | 同方向下的大规模版本线 | 大方向不变的大规模能力升级，例如 `1.2.12` |
| **PATCH**（第 3 位） | 日常迭代 | 功能补强、修复、小改进；默认只改这一位，例如 `1.1.12` → `1.1.13` |

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
状态目录：`%LOCALAPPDATA%\ChatGPTTools\`（旧版 `CodexSkin` / `CodexSkinManager` 会自动合并）。

---

## 打包

```bash
npm run build
```

产物位于 `src-tauri/target/release/bundle/`（NSIS / MSI / 可执行文件等，视平台而定）。

打包前请确认：

- `engine/` 与**内置皮肤**已列入 `bundle.resources`（见下）
- `bundle.active` 为 `true`（安装包 / DMG 才会生成）

### 内置皮肤策略

| 环境 | 皮肤来源 |
|------|----------|
| **开发** `npm run dev` | 仓库完整 `skins/`（含全部主题，便于调试） |
| **安装包** `npm run build` | 仅 **`qingkong`**（`beforeBuildCommand` → `scripts/stage-bundle-skins.mjs`） |
| **其余皮肤** | 运行时从 **打包时注入** 的云端 catalog 拉取（`cloudDownloadSkin` 缓存到本机） |

生产云端 baseUrl / updater endpoints **不写进仓库**，与私钥一样只在打包阶段注入（`keys/release.env` 或 CI Secrets）。见 [`keys/README.md`](keys/README.md)。

覆盖内置列表（构建时）：

```powershell
$env:CODEX_SKIN_BUNDLE_SKINS = "qingkong"
npm run build
```

本地联调 CDN 时：

```powershell
$env:CODEX_SKIN_CLOUD_URL = "http://127.0.0.1:8788/v1"
npm run dev
```

---

## GitHub Releases（自动构建）

采用 **「先发 Release → CI 挂资产」** 模式，而不是 push tag 再自动建 Release。

应用内更新默认读：

```text
https://github.com/<owner>/<repo>/releases/latest/download/latest.json
```

（CI 用 `GITHUB_REPOSITORY` 自动注入；本机用 `npm run stamp:repo` 或编辑 `src/repo-meta.json`。）

### 仓库与首次接入

- 源码仓库：https://github.com/joshuamarke/chatgpt-tools  
- 应用内更新默认：https://github.com/joshuamarke/chatgpt-tools/releases/latest/download/latest.json  
- 关于页 GitHub 按钮指向同一仓库（`src/repo-meta.json`）

推送前请在 **Settings → Secrets and variables → Actions** 配置：

- **`TAURI_SIGNING_PRIVATE_KEY`**（`npx tauri signer generate -w keys/chatgpt-tools.key --ci` 生成的私钥全文）  
- 可选：`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`、独立 CDN 的 `CODEX_SKIN_CLOUD_URL` / `TAURI_UPDATER_ENDPOINTS`

推 `main` 后 **PR build** 应冒烟通过；再按下方步骤发首个 Release。详情见 [`keys/README.md`](keys/README.md)。

### 工作流

| Workflow | 触发 | 作用 |
|----------|------|------|
| [`.github/workflows/release-assets.yml`](.github/workflows/release-assets.yml) | `release: published` | 构建并上传 Windows / macOS 安装包，再生成 `latest.json` |
| [`.github/workflows/pr-build.yml`](.github/workflows/pr-build.yml) | PR / `main` push | 冒烟构建，产物仅作为 Actions Artifact |

### 发版步骤

1. **升版本**（三处保持一致）  
   - `package.json`  
   - `src-tauri/tauri.conf.json`  
   - `src-tauri/Cargo.toml`
2. **写更新日志**：在 [`CHANGELOG.md`](CHANGELOG.md) 顶部增加本版条目。
3. **提交并打 tag**：

```bash
git add -A
git commit -m "chore: release v1.1.13"
git tag v1.1.13
git push origin main --tags
```

4. 在 GitHub **Releases → Draft a new release**：
   - Tag 选 `v1.1.13`
   - 标题如 `ChatGPT Tools v1.1.13`
   - 描述粘贴 `CHANGELOG.md` 中对应段落（这就是 Releases 页显示的更新日志）
   - 选择 **Publish release**（不要只存 Draft；workflow 只监听 `published`）
5. 确认 Actions Secret **`TAURI_SIGNING_PRIVATE_KEY`** 已配置（见上）。  
   云端 CDN **不是**必需；不配时 updater 自动指向本仓库 Releases 的 `latest.json`。
6. 等待 **Release assets** 跑完。资产会自动挂到该 Release：

| 资产 | 说明 |
|------|------|
| `ChatGPTTools-{ver}-windows-x64-setup.exe` | Windows NSIS **安装包**（应用内更新用） |
| `ChatGPTTools-{ver}-windows-x64-setup.exe.sig` | 安装包 minisign 签名 |
| `ChatGPTTools-{ver}-windows-x64-portable.zip` | Windows **免安装**便携版（解压即跑，无需 setup） |
| `ChatGPTTools-{ver}-windows-x64.msi` | Windows MSI（若生成） |
| `ChatGPTTools-{ver}-macos-arm64.dmg` (+ `.sig`) | Apple Silicon |
| `ChatGPTTools-{ver}-macos-x64.dmg` (+ `.sig`) | Intel Mac |
| `latest.json` | **Tauri updater** 静态清单（多 endpoint 读取） |

### 应用内更新（`tauri-plugin-updater`）

- 关于页「检查更新」→ 官方 updater 拉取 `latest.json`
- **默认 endpoint**：本仓库 `releases/latest/download/latest.json`（打包时由 `inject-release-config` 写入）
- 可选：`TAURI_UPDATER_ENDPOINTS` 覆盖或前置 CDN 镜像（多地址 fallback）
- 仓库内 `tauri.conf.json` 的 `endpoints` 保持为空，只在打包 overlay 合并
- 验签：`createUpdaterArtifacts` + CI 私钥签发 `.sig`
- 关于页「查看开源协议」旁 **GitHub** 按钮 → 仓库主页（`src/repo-meta.json`）

`latest.json`（Tauri 格式）形如：

```json
{
  "version": "1.1.13",
  "notes": "…Release 描述…",
  "pub_date": "2026-07-30T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "…",
      "url": "https://github.com/<owner>/<repo>/releases/download/v1.1.13/ChatGPTTools-1.1.13-windows-x64-setup.exe"
    },
    "darwin-aarch64": { "signature": "…", "url": "…-macos-arm64.dmg" },
    "darwin-x86_64": { "signature": "…", "url": "…-macos-x64.dmg" }
  }
}
```

> 代码签名（Authenticode / Apple notarization）与 updater minisign 是两回事；未做系统级签名时，用户首次打开仍可能被 SmartScreen / Gatekeeper 拦截。

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
├── scripts/                     # 构建 / 发版 / 契约与冒烟（无探针杂项）
├── package.json
└── README.md
```

## 引擎 v1.1.10（载荷与多皮肤）

引擎内核与 Tauri 多皮肤管理：

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
| [docs/UNINSTALL.md](./docs/UNINSTALL.md) | 干净卸载 |

---

## License

MIT（内置同人立绘仅供学习美化，请勿商用未授权素材）。
