# 开发环境搭建

## Windows

1. 安装 [Node.js 18+](https://nodejs.org/)  
2. 安装 [Rustup](https://rustup.rs/)，然后：

```powershell
rustup default stable
rustc --version
cargo --version
```

3. 安装 **Visual Studio Build Tools**（C++ 桌面开发工作负载），供 MSVC 链接  
4. 确认 **WebView2 Runtime**（Win10/11 一般已有）  
5. 进入项目：

```powershell
cd E:\临时项目\codex-skin
npm install
npm run dev
```

### 常见问题

| 现象 | 处理 |
|------|------|
| `Missing manifest in toolchain` | `rustup toolchain uninstall stable` 后 `rustup toolchain install stable` |
| `link.exe not found` | 安装 VS Build Tools / 在「x64 Native Tools」终端编译 |
| `未找到 Node.js` | apply/status/restore/import/export（含冷启动与刷新保持）走 Rust，无需 Node；设计壁纸或 `CODEX_SKIN_FORCE_NODE=1` 仍可能需 Node 18+ |
| 想强制 Node / 关原生 | `CODEX_SKIN_FORCE_NODE=1` 或 `CODEX_SKIN_NATIVE=0` |
| 窗口白屏 / API 不可用 | 见下方「白屏排查」；必须用 `npm run dev` 或安装包，勿双击 `index.html` |
| 换肤失败 / 无调试口 | 勾选自动重启，先完全退出 ChatGPT 再应用 |
| `adm-zip` 找不到 | 在项目根执行 `npm install`（引擎与 UI 共用 root `node_modules`） |
| 改名 crate 后编译慢 | `Cargo.toml` 包名变更后首次会全量重编译，属正常 |

### 白屏排查（`npm run dev`）

1. **确认是 Tauri 窗口而非浏览器**：直接打开 `src/index.html` 时 `__TAURI__` 不存在，界面可能空白或仅 toast。  
2. **status 预览图过大**：`status` 会把立绘转成 base64；单张 &gt; ~1.2MB 会跳过，总预览有上限，避免 WebView IPC 卡死。  
3. **引擎/Node 失败**：应显示「引擎未就绪」卡片而非纯白；若仍纯白，看终端 `cargo`/`tauri` 错误与 WebView2 是否可用。  
4. **capability 窗口 label**：须为 `main`（`tauri.conf.json` 已显式配置）。  
5. **状态目录**：`%LOCALAPPDATA%\ChatGPTTools\`

## macOS

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cd /path/to/codex-skin
npm install
npm run dev
```

## 仅测引擎

```bash
node engine/cli.mjs status
node engine/cli.mjs detect
node engine/cli.mjs apply --skin-id dream --restart true
node engine/cli.mjs restore
```

## 日志位置

状态根目录下：

- `diag.log` — 管理器诊断  
- `injector.log` / `injector-error.log` — 注入守护  
- `state.json` — 当前皮肤与 injector PID  
- `settings.json` — 手动指定的客户端路径  

## 图标

当前 `src-tauri/icons` 使用 `logo.png` 占位。正式发版建议：

```bash
npm run tauri -- icon logo.png
```

会生成各尺寸 icns/ico。
