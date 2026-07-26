# 会话管理

管理本机 **ChatGPT / Codex 桌面端** 与 **Grok Build** 的历史会话，不修改官方安装包。

- Codex：移植自 CodexPlusPlus Manager（SQLite 列表 / 删除 / 备份 / 导出 / provider 修复 / index 清理）
- Grok：移植自 cc-switch `session_manager::providers::grokbuild`（`~/.grok/sessions` 扫描 / 删除 / Markdown 导出）

**当前阶段：Phase 2 + Grok Tab**（Codex 全能力；Grok 列表 / 删除 / 导出）。

## 1. 用户入口

GUI 左侧侧边栏 → **会话管理**。页内 **Tab** 切换来源：

| Tab | 说明 |
|-----|------|
| **ChatGPT / Codex** | 本地 SQLite + rollout；可撤销删除、provider 修复、index 清理 |
| **Grok Build** | `~/.grok/sessions`；导出 Markdown；删除为永久移除会话目录（无撤销） |

| 操作 | Codex | Grok |
|------|-------|------|
| 刷新 / 自动加载 | 合并多库、按更新时间倒序分页（默认 50 条）；列表工具栏「刷新」 | 扫描 summary.json，按活跃时间倒序分页 |
| 按项目分组 / 筛选 | 本页按 `cwd` 分组（**默认折叠**，点击展开）；搜索标题/路径/ID；下拉筛项目 | 同左 |
| 路径显示 | 去掉 Windows `\\?\` 扩展前缀（如 `\\?\E:\…` → `E:\…`） | 同左 |
| 复制 Resume | `codex resume <id>` | `grok --resume <id>` |
| 导出 Markdown | 对话框保存；不修改本地数据 | 从 `chat_history.jsonl` 导出 |
| 单删 / 多选批量删除 | DB + rollout + 备份 | 删除会话目录（不可撤销） |
| 撤销删除 | 顶栏撤销条（最近一次 token） | — |
| provider 修复 / index 清理 | 有 | — |
| 分页 | 有 | 有 |

本阶段**不支持**在工具内「新建 / 打开 / 切换」会话（仍由官方客户端完成）。

## 2. 数据源

### 2.1 Codex / ChatGPT

| 路径 | 含义 |
|------|------|
| `$CODEX_HOME` 或 `~/.codex` | Codex 家目录 |
| `{home}/sqlite/*.{db,sqlite,sqlite3}` | 含 `threads` / `automation_runs` 等 |
| `{home}/state_5.sqlite` | 历史库 |
| `{home}/sessions/**/rollout-*.jsonl` | 对话 transcript |
| `{home}/session_index.jsonl` | 会话索引（清理目标） |
| `{home}/config.toml` | 解析当前 / 可选 provider 列表 |
| `$CODEX_SQLITE_HOME` | 可选，覆盖 SQLite 搜索根 |

环境变量 `CODEX_HOME` 若指向有效目录则优先使用。

### 2.2 Grok Build

| 路径 | 含义 |
|------|------|
| `$GROK_HOME` 或 `~/.grok` | Grok 家目录 |
| `{home}/sessions/**/summary.json` | 会话元数据（id / cwd / title / 时间） |
| `{home}/sessions/**/chat_history.jsonl` | 对话内容 |
| `{home}/archived_sessions/**` | 归档会话（若存在，列表中标记为已归档） |

## 3. 备份

### 3.1 删除撤销备份

删除时在应用状态目录写入 undo 备份（JSON）：

- Windows：`%LOCALAPPDATA%\ChatGPTTools\session-backups\`
- macOS：`~/Library/Application Support/ChatGPTTools/session-backups/`
- Linux：`~/.local/share/ChatGPTTools/session-backups/`

备份含相关表行快照与 rollout 文件 base64（若可读）。GUI 通过 `undoToken` 调用撤销；token 仅在当次界面会话中保留最近一次成功删除。

### 3.2 Provider / 索引清理备份

历史 provider 修复与 `session_index` 清理会在 Codex home 下写入：

`{CODEX_HOME}/backups_state/provider-sync/<timestamp>/`

含改动前文件快照与 `metadata.json`。应用会轮转旧备份，避免无限增长。

## 4. Markdown 导出

- 从候选 DB 定位 thread，解析关联 rollout / 事件写入 Markdown。  
- 默认文件名由标题 + session id 生成（Windows 非法字符已替换）。  
- 通过系统「另存为」对话框选择路径；用户取消时返回 `canceled: true`，不报错。

## 5. Provider 历史修复

| 步骤 | 行为 |
|------|------|
| 加载目标 | `load_provider_sync_targets`：当前 config + 历史出现过的 provider id |
| 执行 | `sync_providers_now`：改写 session_meta / SQLite `model_provider` 等历史标记 |
| 目标为空 | 使用 config 中的当前 provider |
| 跳过 | Codex home 不存在、无变更、或文件被锁等 → `status: skipped` + 说明 |

建议在**客户端完全退出**后执行，降低文件锁与状态不一致风险。

## 6. session_index 孤儿清理

| 步骤 | 行为 |
|------|------|
| 预览 | 对比 live thread id 与 `session_index.jsonl`，列出仅索引存在的候选 |
| 应用 | 必须携带预览时的 `snapshotSha256`；可选勾选 `threadIds` |
| 安全 | 哈希不匹配则拒绝（文件已变）；写备份后原子写回 |

## 7. 代码与 IPC

| 层 | 路径 |
|----|------|
| 发现 / 存储 / 导出 / 同步 | `src-tauri/src/sessions/` |
| 命令 | 见下表 |
| 前端 API | `src/features/sessions/sessions-api.js` → `window.sessionAPI` |
| 视图 | `src/features/sessions/sessions-view.js` · `#sessionsView` |

### Tauri 命令

| 命令 | 用途 |
|------|------|
| `list_local_sessions` | Codex 分页列表（`async` + `spawn_blocking`，不阻塞 GUI 主线程） |
| `delete_local_session` | Codex 删除 + 备份 |
| `undo_local_session` | 按 token 撤销 |
| `export_local_session_markdown` | Codex 导出 Markdown |
| `load_provider_sync_targets` | provider 下拉数据（`async` + `spawn_blocking`） |
| `sync_providers_now` | 执行历史 provider 修复 |
| `preview_session_index_cleanup` | 索引孤儿预览 |
| `apply_session_index_cleanup_cmd` | 应用索引清理 |
| `session_paths_info` | Codex / Grok 路径诊断 |
| `list_grok_sessions` | Grok 分页列表（`async` + `spawn_blocking`） |
| `delete_grok_session` | 删除 Grok 会话目录 |
| `export_grok_session_markdown` | Grok 导出 Markdown |

进入「会话管理」时先渲染页面骨架与「正在读取…」状态，再在后台拉列表；快速切换 Tab / 刷新会丢弃过期请求结果。

### `sessionAPI`（camelCase）

```js
// Codex
sessionAPI.list({ offset, limit })
sessionAPI.delete({ sessionId, title?, dbPath? })
sessionAPI.undo({ undoToken, dbPath? })
sessionAPI.exportMarkdown({ sessionId, title?, dbPath? })
sessionAPI.loadProviderTargets()
sessionAPI.syncProviders({ targetProvider? })
sessionAPI.previewIndexCleanup()
sessionAPI.applyIndexCleanup({ snapshotSha256, threadIds? })
sessionAPI.paths()
// Grok
sessionAPI.listGrok({ offset, limit })
sessionAPI.deleteGrok({ sessionId, title?, sourcePath? })
sessionAPI.exportGrokMarkdown({ sessionId, title?, sourcePath? })
```

### 列表字段

`id` · `title` · `cwd` · `modelProvider` · `archived` · `updatedAtMs` · `rolloutPath` · `dbPath`

Grok 行中 `rolloutPath` 为 `summary.json` 路径；`dbPath` 为空。

### 删除结果要点

`status` · `sessionId` · `message` · `undoToken` · `backupPath`

## 8. 风险与建议

- 删除依赖本地备份与 GUI 撤销 token；官方客户端无「一键恢复」。  
- 若客户端正打开该会话，可能锁库或状态不一致——**先关窗口再删 / 再修**。  
- 仅操作本机路径；不上传会话内容。  
- Schema 随官方版本变化时，依赖列探测与多 schema（threads / automation_runs / generic sessions）。  
- 索引清理与 provider 同步会改 Codex home 内文件；务必确认备份目录可写。

## 9. 与皮肤引擎的关系

| | 皮肤 | 会话 |
|--|------|------|
| 数据 | 应用 state + CDP 注入 | Codex home SQLite / rollout / index |
| 是否依赖宿主进程 | 换肤通常需要 | 列表/删除/导出不依赖 CDP；修复/清理建议退出客户端 |
| IPC | `skinAPI` | `sessionAPI` |

二者在侧栏并列；脚部「选择客户端 / 状态 pills」全局保留，便于同时观察宿主是否在线。

## 10. 后续（可选）

- 页内 inject 快捷操作  
- 导出格式扩展（纯文本 / JSON）  
- 撤销条支持多条历史 token  
- 批量导出  

详见 [../architecture/features.md](../architecture/features.md)。
