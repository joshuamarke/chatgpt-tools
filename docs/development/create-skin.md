# 新建皮肤方法

本文说明如何在 **ChatGPT Tools** 中新增一款内置皮肤，或导出为可导入的 `.cgskin` 包。

> **引擎 v2 三层模型**（详见 [skin-contract.md](./skin-contract.md)）：  
> 1. **框架**提供自适应全窗 / 原生控件 / 建议卡 / 壁纸布局**能力基线**（`renderer-core` + `immersive-skin.css`）  
> 2. **契约**是制作约定——遵守即可在不破坏原生控件/建议卡/全窗壁纸的前提下自定义  
> 3. **个性化**由皮肤在 `skins/<id>/` **自己实现**；引擎核心不强制覆盖或限制皮肤 CSS  
>
> 新增皮肤只需 **CSS + 立绘 + `plugin.json` + `skin.json`**。  
> **不要**写 `renderer-inject.js`，不要改 core；全窗默认能力已由框架提供，皮肤在其上细化即可。

## 1. 目录结构

在 `skins/` 下新建目录（目录名建议与 `id` 一致，小写字母/数字/`-`/`_`）：

```text
skins/my-skin/
  skin.json
  assets/
    my-skin.css            # 样式（选择器绑定 markers.rootClass）
    plugin.json            # 装饰层：chromeHtml / 版本
    my-art.jpg             # 立绘 / 壁纸（可为高质量原图；硬上限 16 MB / 16384px / 50MP）
    screenshot.png         # 工具界面卡片缩略图（务必提供，推荐 < 500KB）
```

> **缩略图**：GUI **必须优先** `assets/screenshot.*`；没有时才回退 `art`（大原图会拖慢列表）。  
> **立绘质量**：引擎 **支持大尺寸原图** 以获得更好的背景效果。shell（CSS）会先注入成功；立绘走第二阶段，CDP 超时随体积放大。  
> **格式**：JPEG/WebP 通常更合适；PNG 原图亦可，只要不超过硬限。

## 2. skin.json

```json
{
  "id": "my-skin",
  "name": "我的皮肤 · 显示名",
  "nameEn": "My Skin",
  "version": "2.0.0",
  "description": "一句话介绍",
  "tags": ["自定义", "示例"],
  "previewGradient": "linear-gradient(135deg, #fff 0%, #88f 100%)",
  "accent": "#6688FF",
  "appearance": "auto",
  "art": {
    "focusX": 0.72,
    "focusY": 0.45,
    "safeArea": "left",
    "taskMode": "ambient"
  },
  "assets": {
    "css": "assets/my-skin.css",
    "plugin": "assets/plugin.json",
    "art": "assets/my-art.jpg",
    "artMime": "image/jpeg"
  },
  "markers": {
    "rootClass": "codex-my-skin",
    "homeClass": "my-skin-home",
    "styleId": "codex-my-skin-style",
    "chromeId": "codex-my-skin-chrome",
    "stateKey": "__CODEX_MY_SKIN_STATE__",
    "disabledKey": "__CODEX_MY_SKIN_DISABLED__",
    "artVar": "--my-skin-art"
  },
  "desktopTheme": {
    "appearanceTheme": "light",
    "appearanceLightCodeThemeId": "codex",
    "appearanceLightChromeTheme": "{ accent = \"#6688FF\", contrast = 64, fonts = { code = \"SF Mono\", ui = \"PingFang SC\" }, ink = \"#1a1a2e\", opaqueWindows = true, semanticColors = { diffAdded = \"#BCE8CF\", diffRemoved = \"#F7B8CE\", skill = \"#88aaff\" }, surface = \"#F7F8FF\" }"
  }
}
```

### 字段说明

| 字段 | 说明 |
|------|------|
| `id` | 唯一 ID；用户导入同 id 会覆盖内置 |
| `appearance` | `auto`（跟随 Codex/系统）\| `light` \| `dark` |
| `art.*` | 自适应构图：`focusX/Y`（0..1）、`safeArea`、`taskMode` |
| `assets.plugin` | **必需**；共享 runtime 的装饰配置 |
| `assets.artMime` | 可选提示；实际 MIME 以文件魔数为准 |
| `markers.*` | 必须与 CSS 中的类名 / 变量一致 |
| `desktopTheme` | 可选；写入 `~/.codex/config.toml` 的 **精确** `[desktop]` 段外观（同文件里的 `[desktop.open-in-target-preferences]` 等子表会原样保留）。亮色皮肤写 `appearanceLight*`；**暗色皮肤必须写 `appearanceDarkCodeThemeId` + `appearanceDarkChromeTheme`**（引擎会同步写入）。宿主只在**进程启动**时读 config，因此需勾选「自动重启」或手动重启客户端后 chrome 色才生效。窗口最小化/最大化/关闭与系统设置弹窗的 chrome 色走这组 key，**不是**皮肤 CSS 能单独控制的 |

引擎校验：`id`/`name`、`assets.css`+`art`+`plugin`、markers 关键字段、资源存在、立绘 ≤ 16MB。

## 3. plugin.json（皮肤专属装饰）

```json
{
  "version": "2.0.0",
  "chromeHtml": "<div class=\"my-brand\"><b>标题</b><small>副标题</small></div>",
  "skipAnalysis": false
}
```

| 字段 | 说明 |
|------|------|
| `chromeHtml` | 注入到 `markers.chromeId` 节点内的 HTML；**文案/IP 只写这里** |
| `skipAnalysis` | `true` 时不做 48px Canvas 采样 |
| `version` | 写入运行态，便于调试 |

共享 runtime 会：

- 注册到 `__CHATGPT_TOOLS_SKIN_REGISTRY__`（切皮肤自动卸下其他套）
- shell-guard：有主内容壳才上皮肤；侧栏可缺失
- rAF + 写前 diff，降低 DOM 抖动
- `appearance: auto` 跟随原生 `color-scheme` / 系统
- **两阶段注入**：先 shell（CSS/chrome），soft 校验通过后再 `applyArt` 贴立绘（作者无需改任何东西）
- **热换**：引擎会跨皮肤复用 watch 进程与页面 host；作者无需改任何东西

**新增皮肤不要改 `engine/runtime/renderer-core.js`。**

## 4. CSS：在框架基线上自己实现个性化

引擎在组装 payload 时：

1. **先**写入 `engine/runtime/immersive-skin.css`（框架能力基线）  
2. **再**追加你的 `assets/*.css`（皮肤自实现；引擎不覆盖、不限制）

因此：**全窗壁纸 / 原生控件 / 建议卡默认能力由框架提供**；具体长相由皮肤 CSS 自己写。  
若希望不破坏原生控件与建议卡体验，按契约约定制作即可。完整说明见 **[skin-contract.md](./skin-contract.md)**。

### 4.1 个性化可以写什么

```css
/* 1) Token：基线与皮肤都可消费 */
:root.codex-my-skin {
  --dream-accent: #6688ff;
  --dream-text: #1a1a2e;
  --dream-canvas: #f7f8ff;
  --dream-sidebar: #eef0fa;
  --dream-surface-raised: #fff;
  --dream-line: #c8cad8;
}

/* 2) 任意自定义样式（作者自实现） */
html.codex-my-skin.dream-art-standard aside.app-shell-left-panel { /* … */ }
html.codex-my-skin.dream-art-standard main.main-surface { /* … */ }
html.codex-my-skin.dream-art-wide .composer-surface-chrome { /* 可覆盖基线 … */ }

/* 3) 装饰 chrome — 建议不抢点击 */
#codex-my-skin-chrome { pointer-events: none !important; }
```

### 4.2 契约约定（建议遵守，引擎不强制）

| 约定 | 说明 |
|------|------|
| 原生控件可用 | 建议勿改 `header.app-header-tint` 的 `position` / `top` / `z-index` |
| 侧栏可缺失 | shell-guard 只要求主内容壳；勿把 `aside` 当皮肤存活条件 |
| 装饰少抢事件 | chrome 建议 `pointer-events: none` |
| 建议卡可读 | 建议用 `var(--dream-text)`；深色壳避免近黑字 |
| 整窗壁纸观感 | 宽图默认由框架画 body；卡片内二次 cover 整图会打断连续感 |
| 兼顾全窗时 | 不透明侧栏/主区可挂 `.dream-art-standard` |

### 4.3 变量与 class（框架写入）

- 背景：`var(<markers.artVar>)`，同时有 `--dream-art`  
- 构图：`--dream-art-position`、`--dream-focus-x/y`、`--dream-accent`、`--dream-image-luma`  
- class：`dream-art-wide` \| `dream-art-standard`，`dream-theme-light` \| `dark`，`dream-task-*`，`dream-safe-*`  
- 标记：`data-skin-contract="full-window"`

### 4.4 壁纸与自检

- 全窗沉浸时优先 **2560 × 1440** 纯背景；左侧安全区；少把 UI/水印烤进图  
- 列表用 `screenshot.*`；立绘可大图（硬限 16MB / 16384px / 50MP）  
- 自检：侧栏收起、建议卡、任务侧栏关闭、composer、插件/PR、宠物窗  

**不要改** `renderer-core.js`。全局全窗**默认能力**由维护者改 `immersive-skin.css`；单皮肤外观只改 `skins/<id>/`。

## 5. 本地验证

```powershell
cd E:\demo\chatgpt-tools
npm run test:engine
node engine/cli.mjs check-payload --skin-id my-skin
node engine/cli.mjs status
# 有 ChatGPT 调试口时：
node engine/cli.mjs apply --skin-id my-skin --restart true
node engine/cli.mjs verify --skin-id my-skin
```

## 6. 导出 / 导入

- GUI：卡片「导出」→ `.cgskin`
- CLI：`export-skin` / `import-skin` / `inspect-skin`
- 用户目录：`%LOCALAPPDATA%\ChatGPTTools\skins\<id>\`

导入时引擎会删除包内残留的 `renderer-inject.js`，并去掉 `assets.inject` 字段。

## 7. 自定义壁纸（design-wallpaper）

- 复制基座皮肤 CSS/markers/plugin  
- 写入用户图与 `appearance` / `art.*`  
- 追加 designer CSS 覆盖  

源图注入上限 **16 MB**。

## 8. 状态目录

| 环境 | 路径 |
|------|------|
| 当前 | `%LOCALAPPDATA%\ChatGPTTools\` |
| 暂停标记 | `paused.flag` |
| 运行时皮肤副本 | `runtime-skins\`（内容变更会自动重拷） |
