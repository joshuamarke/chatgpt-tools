# 功能域规划（ChatGPT Tools）

ChatGPT Tools 是面向 ChatGPT / Codex **桌面端**的本机工具箱。皮肤引擎是当前最成熟的功能之一，但**不是**唯一主线；后续会按「功能域」继续扩展。

## 1. 功能矩阵

| 功能域 | 用户价值 | 代码落点 | IPC | 状态 |
|--------|----------|----------|-----|------|
| **Skins** | 换肤、导入导出、自定义壁纸、云目录 | `engine/` · `skins/` · `src-tauri/cdp/` · `cloud/` · `src/skin-api.js` | `window.skinAPI` | 已落地 |
| **Sessions** | 浏览 / 导出 / 清理本机 Codex 与 Grok 会话；Codex 另含 provider 修复与 index 清理 | `src-tauri/sessions/` · `src/features/sessions/` | `window.sessionAPI` | Phase 2 + Grok Tab |
| **Providers** | 渠道供应商：预设添加、校验启用、Codex 双写 / Grok MCP 保留、live 状态 | `src-tauri/providers/` · `src/features/providers/` | `window.providerAPI` | 可用闭环 |
| **Future** | 更多本机工具 | `src-tauri/<domain>/` · `src/features/<domain>/` | 独立 `*API` | 规划中 |

## 2. 边界规则

1. **皮肤会话 ≠ 聊天会话**  
   - 皮肤：`state.json` / `paused.flag` / apply 生命周期  
   - 聊天：`~/.codex` SQLite + rollout；Grok：`~/.grok/sessions`  

2. **功能域互不阻塞**：删会话不依赖 CDP 注入；换肤不读写会话库。  
3. **共享仅限壳层**：侧栏、标题栏、toast、confirm、宿主在线 pills。  
4. **危险操作隔离**：删除会话必须二次确认；备份写在本应用状态目录。  
5. **IPC 分命名空间**：新功能不要塞进 `skinAPI`，避免契约膨胀。

## 3. 目录约定

```text
src/                          # GUI 壳（vanilla JS，无 bundler）
  app.js                      # 导航、主视图切换、通用反馈
  skin-api.js                 # 皮肤 IPC
  features/<domain>/          # 各功能前端模块
src-tauri/src/
  cdp/ · cloud/ · engine.rs   # 皮肤相关
  sessions/                   # 会话功能域
  <future>/                   # 下一个功能域
engine/ · skins/              # 仅皮肤
docs/
  architecture/               # 总览与功能规划
  features/                   # 各功能说明
  development/                # 开发与模块地图
```

新增功能时建议：

1. 在 `src-tauri/src/<domain>/` 实现领域逻辑与 `#[tauri::command]`  
2. 在 `lib.rs` 注册命令  
3. 在 `src/features/<domain>/` 增加 `*-api.js` + 视图逻辑  
4. 在 `index.html` 增加侧栏项与主区 `#…View`  
5. 扩展 `app.js` 的 `setMainView` / 导航  
6. 补 `docs/features/<domain>.md` 与 module-map 条目  

## 4. GUI 导航模型

| `activeView` | 主区 | 皮肤子菜单 |
|--------------|------|------------|
| `skins` | `#skinsView` | 可展开分类 |
| `sessions` | `#sessionsView` | 收起 |
| `providers` | `#providersView` | 收起 |
| `about` | `#aboutView` | 收起 |

侧栏固定项与皮肤分类分组并列；皮肤分类仍由 `skin-categories.json` 驱动。

## 5. 数据目录

| 路径 | 归属 |
|------|------|
| `%LOCALAPPDATA%\ChatGPTTools\` | 应用状态（皮肤 state、pause 等） |
| `%LOCALAPPDATA%\ChatGPTTools\session-backups\` | 会话删除备份 |
| `%LOCALAPPDATA%\ChatGPTTools\providers.json` | 供应商档案（Providers） |
| `%LOCALAPPDATA%\ChatGPTTools\provider-live-backups\` | 切换前 live 配置快照 |
| `$CODEX_HOME` 或 `~/.codex` | Codex 会话 + live 供应商配置（auth.json / config.toml） |
| `$GROK_HOME` 或 `~/.grok` | Grok Build 会话 + live `config.toml` |

## 6. 相关文档

- [overview.md](./overview.md) — 系统分层  
- [../features/skins.md](../features/skins.md) — 皮肤  
- [../features/sessions.md](../features/sessions.md) — 会话  
- [../development/module-map.md](../development/module-map.md) — 改哪里  
