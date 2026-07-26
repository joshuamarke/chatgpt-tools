# 皮肤模板（复制即用）

```bash
# 从仓库根目录
cp -r skins/_template skins/my-skin   # Windows: xcopy /E /I skins\_template skins\my-skin
# 然后全局替换 codex-my-skin / my-skin / MY_SKIN 等 markers
```

## 改哪个文件？（不用去宿主里翻）

| 你想改… | 打开 |
|---------|------|
| 名字、构图、desktop chrome 色 | `skin.json` |
| **GUI 侧栏分类** | `skin.json` → `categories`（如 `["anime","art"]`，id 见 `src/skin-categories.json`） |
| 卡片展示标签（非分类） | `skin.json` → `tags` |
| 颜色、圆角、面板、建议卡样式 | `assets/skin.css` |
| 角落品牌文案 / 装饰 HTML | `assets/plugin.json` |
| 壁纸 | `assets/art.jpg`（≤16MB） |
| 管理器卡片图 | `assets/screenshot.jpg` |
| **宿主元素叫什么**（main/侧栏/顶栏…） | **`engine/runtime/selectors.json`**（全局契约） |
| 全皮肤默认全窗/可读基线 | `engine/runtime/immersive-skin.css` |
| shell-guard / 热换 / 自适应 class | `engine/runtime/renderer-core.js` |

## 禁止

- 不要在皮肤包里放 `renderer-inject.js` / 复制 core  
- 不要改顶栏 `position/top/z-index`  
- 不要在宽图模式下给 main 叠死不透明实心底（除非你刻意不要全窗壁纸）

## 校验

```bash
npm run test:engine
node engine/cli.mjs check-payload --skin-id my-skin
npm run doctor:selectors   # 契约 JSON 自检
```

详见 `docs/development/create-skin.md` 与 `docs/development/skin-contract.md`。
