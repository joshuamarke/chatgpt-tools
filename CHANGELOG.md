# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/)。发版时把对应版本段落粘贴到 GitHub Release 描述中。

## 1.1.12 - 2026-07-29

- 多功能桌面工具箱：皮肤引擎、会话管理、供应商 / 本地路由等模块。
- 换肤热路径以纯 Rust CDP 为主（apply / status / restore 等）。
- 新增 GitHub Actions：`Release assets`（发布 Release 后自动上传 Win/macOS 安装包）与 `PR build artifacts`（PR/main 冒烟构建）。
- GitHub Release 增加 Windows **免安装 portable zip**；应用检查更新使用 **`tauri-plugin-updater`**（`latest.json` + minisign 签名）。

## 如何写下一版

1. 同步三处版本号：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。
2. 在本文件顶部新增 `## x.y.z - YYYY-MM-DD` 段落。
3. 提交后打 tag（如 `v1.1.13`）并在 GitHub 创建 **Published** Release，正文使用本段 changelog。
4. 等待 Actions 把 `.exe` / `.dmg` / `latest.json` 挂到该 Release。
