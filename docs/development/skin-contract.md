# 皮肤框架 · 视觉契约 · 个性化

本项目采用三层模型：

1. **框架**先提供自适应全窗能力（原生控件 / 建议卡 / 全窗壁纸基线）  
2. **契约**是制作时的约定——遵守即可在不破坏上述能力的前提下自定义样式  
3. **个性化**由皮肤自己在 `skins/<id>/` 实现；**引擎核心不做样式强制覆盖**

```text
┌─────────────────────────────────────────────────────────────┐
│  Framework（引擎提供能力，不限制皮肤如何细化）                 │
│  renderer-core.js + immersive-skin.css + payload 拼接        │
│  · shell-guard（主壳才上皮肤；侧栏可缺）                       │
│  · 自适应 class / CSS 变量                                    │
│  · 全窗壁纸 / 建议卡可读 / 单层 composer 等基线样式            │
│  · 原生控件默认可点（pointer-events、不改顶栏几何）             │
└───────────────────────────┬─────────────────────────────────┘
                            │ 契约：制作约定（文档，非引擎硬锁）
┌───────────────────────────▼─────────────────────────────────┐
│  Personalization（skins/<id>/，作者自实现）                    │
│  skin.json · assets/*.css · plugin.json · art · screenshot   │
│  · 任意自定义样式（token、品牌、面板、装饰、布局细化…）         │
│  · 写在框架基线之后，由皮肤自己决定视觉                       │
└─────────────────────────────────────────────────────────────┘
```

## 1. 框架提供什么（能力，不是锁）

| 能力 | 实现位置 | 说明 |
|------|----------|------|
| 主壳才上皮肤 | `renderer-core` shell-guard | 无 `main` 则清理（宠物/透明窗） |
| 侧栏可收起 | core + 基线 CSS | **不**依赖 `aside` 存活 |
| 宽图全窗壁纸 | `immersive-skin.css` + `dream-art-wide` | body 画一次 `--dream-art` 的默认实现 |
| 顶栏/侧栏可点 | 基线 CSS | 默认不改顶栏 `position/top/z-index`；`pointer-events: auto` |
| 建议卡可读 | 基线 CSS | 默认 `color: var(--cg-text)` |
| 单层输入框 | 基线 CSS | 默认去掉原生双层 fade |
| 任务/插件/PR 连续表面 | 基线 CSS | sticky 路由清黑底的默认处理 |
| 拼接顺序 | `payload.mjs` / `payload.rs` | **框架基线在前，皮肤 CSS 在后** |

payload 注入顺序：

```text
/* framework baseline */       ← immersive-skin.css（能力基线）
/* skin personalization */     ← 你的 skins/<id>/assets/*.css（后写，作者自实现）
```

引擎**不会**用契约 CSS 去覆盖皮肤；皮肤可按需细化、扩展甚至覆盖基线规则（同等或更高优先级时后写生效）。  
若希望保留「原生控件 / 建议卡 / 全窗壁纸」体验，请按下面契约制作。

## 2. 契约：制作皮肤时建议遵守（约定）

契约是**作者约定**，不是引擎校验或强制拦截。遵守 = 自定义样式时不破坏关键交互与可读性。

### 2.1 推荐做（个性化）

- 设置 token：`--dream-accent`、`--dream-text`、`--dream-canvas`、`--dream-sidebar`、`--dream-surface-raised`、`--dream-line`、`--dream-ink`
- `plugin.json` 的 `chromeHtml`、字体、品牌文案
- 任意自定义选择器与布局细化（颜色、圆角、阴影、字体、装饰动画…）
- 需要卡片感时，优先挂 `html.<root>.dream-art-standard`，宽图模式少叠不透明实心底
- 装饰层默认 `pointer-events: none`
- `skin.json` 的 `appearance` / `art.focus*` / `safeArea` / `taskMode`

### 2.2 建议避免（会破坏体验，但引擎不拦）

| 建议避免 | 原因 |
|----------|------|
| 改 `header.app-header-tint` 的 `position` / `top` / `z-index` / `transform` | 任务侧栏关闭依赖原生固定顶栏 |
| 在宽图模式下给 `main` / `aside` 强制不透明实心底 | 打断全窗连续壁纸观感 |
| 在首页卡片内二次 `background: var(--art) cover` 整图裁切 | 与「整窗一张图」观感冲突 |
| 装饰 chrome 设 `pointer-events: auto` 且铺满可点区 | 挡住原生按钮 |
| 建议卡写死 `color: #000` 且不跟主题 | 深色壳上可能不可读 |
| 把 `renderer-core.js` / `immersive-skin.css` 拷进皮肤包维护 | 框架应统一升级；皮肤只写自己的 CSS |

### 2.3 推荐选择器写法

```css
/* ✅ token：所有模式生效，基线与皮肤都可消费 */
:root.codex-my-skin {
  --dream-accent: #6688ff;
  --dream-text: #1a1a2e;
  --dream-canvas: #f7f8ff;
  --dream-sidebar: #eef0fa;
  --dream-surface-raised: #ffffff;
  --dream-line: #c8cad8;
}

/* ✅ 面板润色：可自由写；若要兼顾全窗壁纸，可限定 standard */
html.codex-my-skin.dream-art-standard aside.app-shell-left-panel { /* … */ }
html.codex-my-skin.dream-art-standard main.main-surface { /* … */ }

/* ✅ 装饰不抢点击 */
#codex-my-skin-chrome { pointer-events: none !important; }

/* ✅ 需要覆盖基线时：皮肤 CSS 在后，用同等/更高优先级即可 */
html.codex-my-skin.dream-art-wide .composer-surface-chrome {
  /* 你的自定义 … */
}
```

## 3. 个性化皮肤清单（新建 / 改版）

1. 复制 `skins/dream` 结构，改 `id` / markers / 资源路径  
2. 在框架基线之上**自己实现**样式（token + chromeHtml + art + 任意 CSS）  
3. 若目标是全窗沉浸：优先 2560×1440 纯背景、左侧安全区、少把 UI 烤进图  
4. `npm run test:engine` + `check-payload --skin-id <id>`  
5. 实机自检：侧栏收起、建议卡对比度、任务侧栏关闭、composer、插件/PR  

详见 [create-skin.md](./create-skin.md)。

## 4. 框架 / 契约 / 个性化边界

| 改什么 | 谁改 | 影响 |
|--------|------|------|
| DOM 适配 / shell-guard / 生命周期 | 引擎 → `renderer-core.js` | 全部皮肤 |
| 全窗/原生/建议卡**默认能力** | 引擎 → `immersive-skin.css` | 全部皮肤（基线） |
| 视觉契约文档 | 引擎文档 | 指导制作，非运行时锁 |
| 色板、品牌、布局细化、自定义样式 | 皮肤作者 → `skins/<id>/` | 单皮肤，作者自实现 |

**产品方向**：多皮肤 + 共享 core + 框架提供自适应全窗能力；个性化在框架上由皮肤自己细化；引擎不强制限制皮肤 CSS。
