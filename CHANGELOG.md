# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/)。发版时把对应版本段落粘贴到 GitHub Release 描述中。

## Unreleased

## 1.1.15 - 2026-08-19

- 完善 Codex/Grok 协议路由与 Chat Completions 转换，修复供应商名称及模型配置写入兼容性。
- 优化关于页云端广告与赞助卡片挂载、样式注入及后台刷新防护。

## 1.1.14 - 2026-08-15

- 自定义皮肤弹窗「面板透明度」范围改为 0–100%。
- 修复「从本机配置导入」对同一本机配置重复创建档案。
- 修复 Grok 配置文件 `name` 语义对齐 xAI 设置文档。
- 供应商 / 本地路由：优化代理接管与切换路径；官方 Codex 默认模型改为 gpt-5.6-terra。
- 工具箱增强：强制中文、插件解锁、快速启动、Computer Use Guard（默认关闭）。
- 标题栏拆分为启动/重启宿主；「暂停皮肤」仅在注入正常时显示。
- 自定义壁纸改为 Tauri/Rust 实现，不再依赖系统 Node。
- 皮肤注入与自定义壁纸生成路径继续收敛到原生引擎。

## 1.1.13 - 2026-08-03

- 多功能桌面工具箱：皮肤引擎、会话管理、供应商 / 本地路由等模块，不修改客户端通过CDP注入实现。
- 新增 使用第三方接口的时候设置Codex中文界面、解锁Codex插件功能。
- 修改 预设供应商DeepSeek的协议(wire_api)默认选中responses，原生的deepseek v4 pro目前仍是Chat Completions在codex中要responses。
- 优化 供应商设置在写配置文件时候原子替换避免误伤。

## 如何写下一版

1. 同步三处版本号：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`。
2. 在本文件顶部新增 `## x.y.z - YYYY-MM-DD` 段落。
3. 提交后打 tag（如 `v1.1.15`）并在 GitHub 创建 **Published** Release，正文使用本段 changelog。
4. 等待 Actions 把 `.exe` / `.dmg` / `latest.json` 挂到该 Release。
