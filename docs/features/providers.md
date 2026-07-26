# 供应商管理（Providers）

为 Codex 与 Grok Build 管理 API 供应商配置：在本工具中保存档案，启用时写入对应工具的 live 配置文件，形成「添加 → 配置 → 切换 → 真正可用」的闭环。

## 能力

| 操作 | 说明 |
|------|------|
| 列表 | 按应用（Codex / Grok）展示供应商，高亮当前启用项；显示就绪 / live 漂移标记 |
| Live 状态条 | 标题显示「当前启用 · 供应商名」；副文案对比档案与本机是否一致 |
| 渠道预设 | 添加时可选用 OpenRouter、DeepSeek、Kimi、SiliconFlow、xAI 等模板 |
| 添加 | 名称、Base URL、模型、API Key、Codex `wire_api`；可选高级 TOML |
| 连通测试 | 添加/编辑时对 Base URL 做轻量 HTTP 可达性探测（任意 HTTP 响应即视为可达，不校验鉴权） |
| 拉取模型 | 通过 OpenAI 兼容的 `/models` 候选端点拉取可用模型列表 |
| 高级设置 | 自定义 User-Agent、本地代理 Header/Body 覆盖、手写 config.toml |
| Codex 模型映射 | 主表单 Codex 区；启用时生成 `model_catalog_json` 供 `/model` 显示第三方模型名 |
| 保存 / 保存并启用 | 仅存档案，或保存后立即投影到 live |
| 编辑 | 修改档案；当前启用项保存后会重新投影到 live |
| 启用 | 校验完整后写入 live，并更新 `current` 指针；切换前对旧档案做 live 回填 |
| 删除 | 不可删除官方种子与当前启用项 |
| 从 live 导入 | 读取本机现有配置另存为档案 |

## 应用差异

### Codex

- 档案形状：`settingsConfig = { auth, config }`
- Live：`~/.codex/auth.json` + `~/.codex/config.toml`
- **第三方启用**：双写 API Key 到 `auth.json`（`OPENAI_API_KEY`）与 config 中的 `experimental_bearer_token`；`wire_api` 支持 `responses` / `chat`
- **模型映射**（添加/编辑表单 Codex 区，非高级折叠）：
  - 档案 SSOT：`settingsConfig.modelCatalog.models`（完整列表，对齐 Codex++ `model_list`）
  - 启用时生成 `~/.codex/chatgpt-tools-model-catalog.json`
    - **按 slug** 优先复用本机 `models_cache.json` 原生条目（窗口 / 推理等级 / 工具字段）
    - 未知第三方模型再 clone 通用模板，并剥离 freeform tools
    - `effective_context_window_percent = 100`，`auto_compact_token_limit = null`
  - 写入 `config.toml` 的 `model_catalog_json = "chatgpt-tools-model-catalog.json"`（Codex 以该文件为 `/model` **完整列表**）
  - 「拉取模型」在映射表旁，会写入全部模型行；「添加模型」可手动增行
  - 映射为空时回退为上方默认模型一行；改映射后需**完全重启 Codex**
- **高级 config.toml**：
  - 编辑器展示 **完整配置**（档案与 live 取更丰富者，再叠档案路由）
  - 保存时 **overlay 合并**，不会用短模板整文件覆盖并抹掉 MCP / desktop / features
  - 启用写 live 时同样先与现有 `~/.codex/config.toml` 合并
- **官方（`codex-official` / OpenAI Official）**：内置官方渠道（列表用 ChatGPT logo），**不是**本机 live 导入
  - 启用后恢复 Codex 内置路由，清除第三方代理；登录由客户端管理（ChatGPT 订阅或 Platform API）
  - 保留 MCP、plugins、projects、desktop 等本地设置
  - 有第三方 `base_url` / 自定义 `model_provider` 时不算与 live 一致
- 切换前对即将离开的档案做 backfill（从 live 回写 key / config）
- 内置种子每次 load 会刷新官方说明与路由元数据

### Grok Build

- 档案形状：`settingsConfig = { config }`（TOML 字符串）
- Live：`~/.grok/config.toml`
- **官方（`grok-official` / Grok Official）**：内置官方渠道（列表用 Grok logo），默认模型 `grok-4.5`
  - 认证：`grok login` 或 `XAI_API_KEY`；启用后清除第三方中转，保留 UI / MCP
  - 仅有 `[models] default`、无第三方 `base_url` 仍算官方
- 自定义需完整 `[models]` + `[model."<id>"]`（含 `api_key` 或 `env_key`、`base_url`、`api_backend`、`context_window`）
- 写入 live 时尽量 **保留 MCP 段**（`[mcp]` / 相关表）
- 预设「xAI 官方 API」是 **BYOK 第三方档案**，与内置 Grok Official 不同

## 推荐使用路径

1. 打开侧栏 **供应商管理** → 选 Codex 或 Grok Tab  
2. **添加供应商** → 选渠道预设 → 粘贴 API Key  
3. （可选）点 **测试连通** 确认 Base URL 可达；点 **拉取模型** 选择上游模型  
4. **保存并启用**（或先「仅保存」，再在列表点「启用」）  
5. 看工具栏左侧 **当前启用 · 供应商名** 与本机状态是否一致  
6. 重启 Codex / Grok 客户端或 CLI 后验证请求  

若 live 被外部改过，可再次点列表中的「启用」或在编辑页「保存并启用」强制同步。

## 连通测试与模型拉取

| 能力 | 行为 | 说明 |
|------|------|------|
| 测试连通 | `GET base_url` | 收到任意 HTTP 状态码即「可达」；仅 DNS/连接/TLS/超时算失败。可达 ≠ 鉴权正确 |
| 拉取模型 | `GET {base}/v1/models` 等候选 | 需 API Key；对版本段（`/v1`、`/v4`）与 Anthropic 兼容子路径做候选回退（逻辑移植自 cc-switch） |

编辑已有档案时会回显已保存的 API Key（password 输入框，右侧小眼睛 SVG 可切换明文）。模型字段始终可手填；拉取成功后写入第一项模型 id，并用 datalist 提供下拉建议（仅 id，无厂商后缀）。

## 代码落点

| 层 | 路径 |
|----|------|
| Rust 领域 | `src-tauri/src/providers/`（`codex` / `grok` / `presets` / `store` / `commands`） |
| 命令 | `list_providers` · `get_provider` · `add_provider` · `update_provider` · `delete_provider` · `switch_provider` · `import_live_as_provider` · `provider_paths_info` · `list_provider_presets` · `reapply_current_provider`（API 保留，UI 已去掉） · `test_provider_connectivity` · `fetch_provider_models` |
| 探测 | `src-tauri/src/providers/probe.rs`（连通 + 模型列表） |
| GUI | `src/features/providers/providers-api.js` · `providers-view.js` |
| 导航 | 侧栏「供应商管理」· 主区 Tab：Codex / Grok Build |

## 与 cc-switch 的关系

从 cc-switch 迁移了 **Codex / Grok 配置形态与切换语义** 的精简子集，未引入：

- SQLite 多应用数据库、代理接管、故障转移
- MCP / Skills 全量管理 UI、用量脚本、OAuth 托管登录流程
- 商业级全量预设库（本阶段为常用渠道模板 + 用户自定义）

## 注意

1. 切换后通常需重启 Codex / Grok 客户端或 CLI。  
2. 删除档案不会回滚 live 文件。  
3. API Key 在列表中仅显示掩码；编辑表单会回显完整 Key（可切换显示/隐藏）。  
4. Codex 第三方会覆盖 `auth.json` 中的 `OPENAI_API_KEY`；切回官方时若无 OAuth 缓存，需在 Codex 重新登录。  
