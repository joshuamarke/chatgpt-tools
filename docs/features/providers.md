# 供应商管理（Providers）

为 Codex 与 Grok Build 管理 API 供应商配置：在本工具中保存档案，启用时写入对应工具的 live 配置文件，形成「添加 → 配置 → 切换 → 真正可用」的闭环。

## 能力

| 操作 | 说明 |
|------|------|
| **本地路由** | Tab 旁开关；启动本机代理（默认 `127.0.0.1:18964`），将 live `base_url` 指向代理；切换供应商可热生效；关闭后恢复直连。与皮肤共用 `live_config` 文件锁，避免抢写 `config.toml`。**Codex 热切**不重写 `model_providers`（仅 `model` / catalog + 档案 current）；**Grok 热切**不改 live（上游只在路由内切换），开启路由时 `[model."…"].name = "localproxy"` |
| **出口代理** | 路由设置中的全局字段；开启本地路由后，上游转发经此 HTTP/SOCKS5 出口（留空 = 直连，不继承系统代理）；运行中可热更新，改监听端口仍需先关路由 |
| **故障转移** | **默认开启**；独立 `failoverOrder` 队列（与列表排序解耦）；路由设置可添加/排序/移除，队列行可 **切换** 手动启用；**仅本地路由开启时**卡片显示「加入队列」；转发时当前供应商优先 + 熔断。**成功判定不只看 HTTP 2xx**：流式须在「首字节超时」内收到首包，非流式须读完整 body；2xx 语义错误信封与首包超时会计失败并切换下一家 |
| **请求日志** | Tab 旁「日志」按钮；弹窗查看本地路由转发记录（时间 / 应用 / 供应商 / 模型 / 耗时 / Token / 状态）；详情含首字节时间与 input/output tokens；路由设置中「请求日志」默认勾选，**关闭则不落盘**；弹窗可设**保留天数**（默认 7）与一键清空 |
| **路由异常** | live 与接管 flag 不一致时显示「路由异常」与「修复路由」，避免与普通「本机漂移」混淆 |
| **端口检测** | 路由设置内可检测监听端口；占用时建议空闲端口 |
| 列表 | 按应用（Codex / Grok）展示供应商，高亮当前启用项；显示就绪 / live 漂移标记 |
| Live 状态条 | 标题显示「当前启用 · 供应商名」；副文案对比档案与本机是否一致 |
| 渠道预设 | 添加时可选用 OpenRouter、DeepSeek、Kimi、SiliconFlow、xAI 等模板 |
| 添加 | 名称、Base URL、模型、API Key、Codex `wire_api`；可选高级 TOML |
| 连通测试 | 添加/编辑时对 Base URL 做轻量 HTTP 可达性探测（任意 HTTP 响应即视为可达，不校验鉴权） |
| 拉取模型 | 通过 OpenAI 兼容的 `/models` 候选端点拉取可用模型列表 |
| 高级设置 | 自定义 User-Agent、本地代理 Header/Body 覆盖、手写 config.toml |
| Codex 模型映射 | 主表单 Codex 区；启用时生成 `model_catalog_json` 供 `/model` 显示第三方模型名；上下文列内置主流模型窗口预设（如 `grok-4.5`=5M、`gpt-5.6-sol`=372K） |
| 保存 / 保存并启用 | 仅存档案，或保存后立即投影到 live |
| 编辑 | 修改档案；当前启用项保存后会重新投影到 live |
| 启用 | 校验完整后写入 live，并更新 `current` 指针；切换前对旧档案做 live 回填 |
| 删除 | 不可删除官方种子与当前启用项 |
| 从 live 导入 | 读取本机现有配置另存为档案 |

## 请求日志

本地路由开启后，代理会把**元数据**写入本机 SQLite（`%LOCALAPPDATA%\ChatGPTTools\proxy-request-logs.db`），**不记录** API Key 与请求体。

| 项 | 说明 |
|----|------|
| 入口 | 供应商 Tab 旁 **日志**（与「路由设置」并列） |
| 开关 | 路由设置 →「请求日志」；**默认勾选**。关闭后**不再写入**新记录，历史仍可查看 |
| 列表字段 | 时间 · 应用 · 供应商 · 模型 · 耗时 · Token（输入→输出）· 状态；点行可看 path、首字节、错误等详情 |
| 成功/统计 | 熔断与故障转移在「首包/完整 body 校验」后才记成功；从 JSON/SSE 解析 usage 写入 input/output tokens 与 first_token_ms |
| 保留天数 | 日志弹窗上方可设，**默认 7 天**；写入时按天 prune，另有条数硬上限 |
| 清空 | 弹窗「清空」→ 二次确认（danger）→ 删除全部 |
| 故障转移 | **默认开启**；需配置备用队列后才真正多上游切换；队列行「切换」可手动启用（路由开启时热切，并置为 P1） |

## 应用差异

### Codex

- 档案形状：`settingsConfig = { auth, config }`
- Live：`~/.codex/auth.json` + `~/.codex/config.toml`
- **第三方启用（默认保留官方登录）**：只写 `config.toml`（`model_providers.*` + `requires_openai_auth = true` + provider 作用域 `experimental_bearer_token`），**不覆盖** `auth.json` 中的 ChatGPT / Codex OAuth，便于远程操作与官方插件。可在 Codex Tab 下关闭「切换第三方时保留 Codex 官方登录」以恢复旧版 dual-write（`auth.json` 写入 API Key）
- **模型映射**（添加/编辑表单 Codex 区，非高级折叠）— **桌面与 CLI 共用**：
  - 档案 SSOT：`settingsConfig.modelCatalog.models`（完整可选模型列表）
  - **必须把所有要显示/调用的第三方模型写进映射**（DeepSeek / Claude / Gemini / Grok / …）。Codex 一旦挂上 `model_catalog_json`，列表以该文件为权威，不会自动显示未映射的上游模型。
  - 启用时生成 `~/.codex/chatgpt-tools-model-catalog.json`
    - **按 slug** 优先复用本机 `models_cache.json` 原生条目（窗口 / 推理等级 / 工具字段）
    - 未知第三方模型再 clone 通用模板，并剥离 freeform tools；强制 `visibility=list`、`supported_in_api=true`
    - `effective_context_window_percent = 100`，`auto_compact_token_limit = null`
    - 默认 `model` 与映射表自动合并，避免只 seed 一个 GPT
  - 写入 `config.toml` 的 `model_catalog_json = "chatgpt-tools-model-catalog.json"`（CLI `/model` 与桌面列表的 SSOT）
  - **桌面优先（白名单解锁）**：Codex 桌面用 Statsig / `list-models-for-host` 白名单过滤模型；**非白名单 slug 会隐藏或显示为「自定义」**。本工具通过 CDP 注入只 patch **数据层**：
    - `Response.json`、Statsig `available_models`、app-server `list-models-for-host`
    - React **模型菜单**状态中的 model 描述符（`displayName` 等）；**不**遍历侧栏最近对话
    - **不**改写页面 DOM 文本（避免误伤其它控件）
    - **仅第三方**：OpenAI Official / `chatgpt-tools-official` 官方代理路由 **跳过**注入，并清除已有 hook
    - **启用时 Codex 未开也可**：标记 desired，端口出现后自动注入；启动宿主触发 `on_host_ready`
    - **注入成功即停轮询**：页面健康后进入 **stable park**（不再 8s CDP）；仅在供应商切换 / 映射变更 / 宿主就绪时唤醒
    - **慢速兜底**：stable 下约 2 分钟一次轻量探测，仅 SPA 丢补丁时再修一次
    - 官方 / 无映射：完全停用；需调试端口（默认 `9335`）；CLI 只依赖 `model_catalog_json`
  - 「拉取模型」在映射表旁，会写入全部模型行；渠道预设会预填常见多厂商 id
  - 映射为空时回退为上方默认模型一行
- **高级 config.toml**：
  - 编辑器展示 **完整配置**（档案与 live 取更丰富者，再叠档案路由）
  - 保存时 **overlay 合并**，不会用短模板整文件覆盖并抹掉 MCP / desktop / features
  - 启用写 live 时同样先与现有 `~/.codex/config.toml` 合并
- **官方（`codex-official` / OpenAI Official）**：内置官方渠道（列表用 ChatGPT logo），**不是**本机 live 导入
  - 启用后恢复 Codex 内置路由，清除第三方代理；登录由客户端管理（ChatGPT 订阅或 Platform API）
  - 保留 MCP、plugins、projects、desktop 等本地设置
  - 有第三方 `base_url` / 自定义 `model_provider` 时不算与 live 一致
  - **种子 ≠ 启用**：首次打开只插入 Official 档案，不会仅因种子把 `current` 标成官方。load 时按 live 软对齐：live 官方/缺失 → 指向 Official；live 已是第三方且无匹配档案 → `current` 留空（提示导入），避免「使用中 + 本机漂移」误报
- 切换前对即将离开的档案做 backfill（从 live 回写 key / config）
- 内置种子每次 load 会刷新官方说明与路由元数据

### Grok Build

- 档案形状：`settingsConfig = { config }`（TOML 字符串）
- Live：`~/.grok/config.toml`
- **官方（`grok-official` / Grok Official）**：内置官方渠道（列表用 Grok logo），默认模型 `grok-4.5`
  - 认证：`grok login` 或 `XAI_API_KEY`；启用后清除第三方中转，保留 UI / MCP
  - 仅有 `[models] default`、无第三方 `base_url` 仍算官方
  - 与 Codex 相同：种子不自动等于「当前启用」；按 live 软对齐，避免第三方 live 下误标 Official 漂移
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
| 拉取模型 | `GET {base}/v1/models` 等候选 | 需 API Key；对版本段（`/v1`、`/v4`）与 Anthropic 兼容子路径做候选回退 |

编辑已有档案时会回显已保存的 API Key（password 输入框，右侧小眼睛 SVG 可切换明文）。模型字段始终可手填；拉取成功后写入第一项模型 id，并用 datalist 提供下拉建议（仅 id，无厂商后缀）。

## 代码落点

| 层 | 路径 |
|----|------|
| Rust 领域 | `src-tauri/src/providers/`（`codex` / `grok` / `presets` / `store` / `commands`） |
| 本地路由 | `src-tauri/src/proxy/`（`server` 监听 · `forwarder` 上游转发 + 出口代理 · `takeover` 改写 live · `runtime` 启停） |
| 命令 | `list_providers` · `get_provider` · `add_provider` · `update_provider` · `delete_provider` · `switch_provider` · `import_live_as_provider` · `provider_paths_info` · `list_provider_presets` · `reapply_current_provider` · `test_provider_connectivity` · `fetch_provider_models` · `refresh_codex_model_unlock` · `get_proxy_config` / `update_proxy_config`（含 `egressProxy`） |
| 探测 | `src-tauri/src/providers/probe.rs`（连通 + 模型列表） |
| GUI | `src/features/providers/providers-api.js` · `providers-view.js` |
| 导航 | 侧栏「供应商管理」· 主区 Tab：Codex / Grok Build |

### 本地路由数据流

```
Codex/Grok 客户端
  → live base_url（被接管为 127.0.0.1:18964）
  → 本机路由代理（UA / Header 覆盖、故障转移、熔断）
  → [可选] 全局出口代理 egressProxy
  → 供应商档案中的真实上游 base_url + API Key
```

供应商档案侧的「自定义 User-Agent / Header 覆盖」只在本地路由转发时生效；直连启用时由客户端自己发请求。

| 动作 | live 写入 |
|------|-----------|
| 首次开启本地路由 / 修复路由 | 确保 `model_provider = chatgpt-tools-proxy`（或 official 变体）与代理表；**不**把档案的 `model_providers.custom` 覆盖进 live |
| 路由下热切供应商 | **不改** `model_providers`；只改 `model` + catalog 投影 + `providers.json` current |
| 直连启用 / 编辑保存 | 档案与 live 中 `model_providers.*.name` 固定 `OpenAI`（不是供应商显示名） |

## 设计要点

| 能力 | 本项目 |
|------|--------|
| `modelCatalog` → `model_catalog_json` | ✅ 档案为 SSOT，启用时投影到 live |
| 默认保留官方 OAuth | ✅（默认开；可关） |
| 桌面模型白名单 CDP 注入 | ✅（启用 / 皮肤 / 手动刷新） |
| 本地路由接管 | ✅（热切、故障转移、请求日志） |
| 多模型映射 | ✅ 映射表 = 完整可选列表 |

## 注意

1. **映射表 = 完整可选列表**。只填默认 `gpt-5.5` 时，桌面/CLI 不会出现 `grok-4.5` 等其它模型。  
2. 桌面注入依赖 Codex 以调试端口启动（本工具宿主默认 `9335`）。未注入时依赖 catalog + 完全重启。  
3. 删除档案不会回滚 live 文件。  
4. API Key 在列表中仅显示掩码；编辑表单会回显完整 Key（可切换显示/隐藏）。  
5. 关闭「保留官方登录」时第三方会覆盖 `auth.json`，桌面可能只剩官方 GPT 默认列表。  
