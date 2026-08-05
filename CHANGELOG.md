# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/)。发版时把对应版本段落粘贴到 GitHub Release 描述中。

## Unreleased

- **单一路径引擎**：GUI 运行时不再 spawn Node / `engine/cli.mjs`；失败不再静默回退。
- **自定义壁纸**改为 Tauri/Rust 实现（`src-tauri/src/cdp/design.rs`），无系统 Node。
- 资源根发现以 `engine/runtime/renderer-core.js` 为准。
- Release 构建要求 Secret `CODEX_SKIN_CLOUD_URL`（例：`https://cdn.aiku.cc.cd/v1`）以嵌入默认云端 catalog。

## 1.1.13 - 2026-08-03

- 多功能桌面工具箱：皮肤引擎、会话管理、供应商 / 本地路由等模块，不修改客户端通过CDP注入实现。
- 新增 使用第三方接口的时候设置Codex中文界面、解锁Codex插件功能。
- 修改 预设供应商DeepSeek的协议(wire_api)默认选中responses，原生的deepseek v4 pro目前仍是Chat Completions在codex中要responses。
- 优化 供应商设置在写配置文件时候原子替换避免误伤。

## 如何写下一版

1. 同步三处版本号：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。
2. 在本文件顶部新增 `## x.y.z - YYYY-MM-DD` 段落。
3. 提交后打 tag（如 `v1.1.13`）并在 GitHub 创建 **Published** Release，正文使用本段 changelog。
4. 等待 Actions 把 `.exe` / `.dmg` / `latest.json` 挂到该 Release。
