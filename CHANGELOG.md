# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/)。发版时把对应版本段落粘贴到 GitHub Release 描述中。

## 1.1.12 - 2026-07-29

- 多功能桌面工具箱：皮肤引擎、会话管理、供应商 / 本地路由等模块，不修改客户端通过CDP注入实现。
- 换肤热路径以纯 Rust CDP 为主（apply / status / restore 等），主题模式切换需要经过ChatGPT/Codex的配置文件切换，因此dark/light模式切换必须要重启客户端。
- 新增 会话管理，支持ChatGPT/Codex和Grok Build。
- 新增供应商管理，当开启本地路由功能之后可以实现多个第三方中转站热切换。

## 如何写下一版

1. 同步三处版本号：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。
2. 在本文件顶部新增 `## x.y.z - YYYY-MM-DD` 段落。
3. 提交后打 tag（如 `v1.1.13`）并在 GitHub 创建 **Published** Release，正文使用本段 changelog。
4. 等待 Actions 把 `.exe` / `.dmg` / `latest.json` 挂到该 Release。
