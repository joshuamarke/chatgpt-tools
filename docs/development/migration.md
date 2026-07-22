# 迁移说明

## 从 codex-skin-manager（Electron）

| 项 | 说明 |
|----|------|
| `engine/manager.js` | 已增强 `CODEX_SKIN_ROOT` / 状态目录名 |
| 注入 / 导入导出 / 壁纸主题 | 对齐原 Electron 功能 |
| GUI | Tauri 壳 + 原 `src` 交互 |

## 从 Codex Dream Skin（引擎内核，v2）

| 能力 | 落点 |
|------|------|
| 图像元数据 / 像素炸弹 | `engine/image-metadata.mjs` |
| payload 指纹缓存 + **全量 token 替换** | `engine/payload.mjs` |
| CDP 身份 / loopback / 超时 | `engine/injector.mjs` |
| 共享渲染 + shell-guard + 自适应 | `engine/runtime/renderer-core.js` |
| 操作互斥 / 安全 stop injector | `engine/manager.js` |
| soft/hard verify、pause | injector + manager + CLI |
| 皮肤装饰插件化 | `skins/*/assets/plugin.json` **only** |
| 宿主慢启动探测 | `engine/host-probe.js`（2.2） |
| 大图立绘 | staged inject + 缩放 CDP 超时 + 分块 base64 解码 |

**未照搬**：单主题脚本形态、去掉 Tauri GUI、每平台复制 injector。

## 产品与路径差异

1. **产品名**：**ChatGPT Tools**。  
2. **包标识**：`com.chatgpt.tools`。  
3. **状态目录名**：`ChatGPTTools`（旧 `CodexSkin` / `CodexSkinManager` 首次启动合并）。  
4. **npm / crate**：`chatgpt-tools`。  
5. **预览图**：status 时优先 screenshot；**立绘允许大尺寸原图**（UI 勿用原图做列表缩略图）。  

## 环境变量

| 变量 | 含义 |
|------|------|
| `CODEX_SKIN_ROOT` | 资源根 |
| `CODEX_SKIN_STATE_NAME` | 默认 `ChatGPTTools` |
| `CODEX_SKIN_PORT` | CDP 端口 |
| `CODEX_SKIN_NODE` | 指定 node 路径 |
| `CODEX_SKIN_SLOW_SCALE` | 慢机器超时倍率（1–3） |

## 皮肤包格式（protocol 2，无旧 inject）

**必需**：

```text
skin.json
assets/<name>.css
assets/<art>          # 可为高质量原图，硬限 16MB / 16384px / 50MP
assets/plugin.json    # chromeHtml 等
```

**强烈建议**：

```text
assets/screenshot.{png,jpg,webp}   # 列表缩略图（小文件）
```

**已移除（不兼容旧包 inject）**：

- `assets/renderer-inject.js`
- `assets.inject` / `useLegacyInject`（导入时自动剥离）

引擎始终用 **shared core** 组装注入脚本；`payload.mjs` 对占位符做 **split/join 全量替换**，并对组装结果做 `new Function` 语法校验。

## 性能注意

- 注入载荷约 `1.37 × artBytes + css + core`。大原图会拉长 **art 阶段**，但 **shell 阶段**仍应快速成功。  
- 引擎已：fingerprint 缓存、soft once、rAF 写 diff、分析缓存、runtime-skins 内容戳、art 超时按体积缩放、分块 base64 解码。  
- UI 列表请用 `screenshot`；不要用多 MB 立绘做卡片预览。  
- 慢机器：`CODEX_SKIN_SLOW_SCALE=2`，并观察 status.`lifecycle`。

## 历史问题（已修）

1. `String.replace` 只替换 header 注释里的第一处 `__SKIN_*` → 全量替换 + 语法断言。  
2. 慢启动仅用进程列表 → 误报「未打开」→ **host-probe 三信号 lifecycle**。  
3. soft once 被大图 base64 卡住 → shell 先成功，`artPending` 回报。  
