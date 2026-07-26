# macOS 启动链说明

## 默认路径（推荐，免 Node）

Tauri / Rust `src-tauri/src/cdp/launch.rs`：

1. 解析 `CODEX_APP_PATH` 或常见路径（`/Applications`、`~/Applications` 的 ChatGPT/Codex）
2. 优先 **`open -n -a App.app --args --remote-debugging-port=… --remote-debugging-address=127.0.0.1`**
3. 失败再直接 spawn 可执行文件（同样带调试参数）
4. `ensure_debug_port` 用三信号 lifecycle 等到 `app://` 可注入

日常换肤 **不需要** 系统 Node，也不需要 launchd。

## 可选：launchd 调试 Agent

若个别 macOS 版本仍出现「进程起来了但 CDP 口未开」（LaunchServices 丢 flag），可安装可选 Agent：

```bash
chmod +x scripts/macos/install-debug-launch-agent.sh
./scripts/macos/install-debug-launch-agent.sh          # ChatGPT.app
./scripts/macos/install-debug-launch-agent.sh Codex     # Codex.app
launchctl kickstart -k "gui/$(id -u)/com.chatgpt-tools.remote-debug"
```

卸载：

```bash
./scripts/macos/install-debug-launch-agent.sh --unload
```

| 项 | 值 |
|----|-----|
| Label | `com.chatgpt-tools.remote-debug` |
| Plist | `~/Library/LaunchAgents/com.chatgpt-tools.remote-debug.plist` |
| 端口 | `CODEX_SKIN_PORT` 或 `9335` |
| 日志 | `~/Library/Logs/chatgpt-tools-remote-debug*.log` |

**注意**：Agent 与 GUI 同时抢端口时，以先占用者为准；调试完建议 unload。

## 与 Codex Dream Skin 的差异

| | Dream Skin | 本项目 |
|--|------------|--------|
| 日常启动 | launchd + 官方 bundle Node | `open -a` + **Rust CDP** |
| Node | 用官方 `cua_node` | 主路径 **不需要** |
| 可选加固 | 产品内建 | `scripts/macos/install-debug-launch-agent.sh` |

## 相关代码

- `src-tauri/src/cdp/launch.rs` — `launch_macos_app` / `ensure_debug_port`
- `src-tauri/src/cdp/host.rs` — macOS 状态目录与 pgrep
- `docs/development/module-map.md` — 模块索引
