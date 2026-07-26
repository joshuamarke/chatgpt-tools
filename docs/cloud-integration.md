# 本地客户端 ↔ 云端 CDN 对接说明

本文描述 **ChatGPT Tools**（本仓库）如何接入 **chatgpt-tools-cdn** 云端服务：公告位、版本检查、皮肤目录与安全下载缓存。

契约以 CDN 仓文档为准：

- 协议：`protocol: 1`
- CDN 仓库建议路径：`E:\demo\chatgpt-tools-cdn`
- 详细 HTTP 字段：[chatgpt-tools-cdn/docs/api.md](../../chatgpt-tools-cdn/docs/api.md)
- CDN 侧客户端约定：[chatgpt-tools-cdn/docs/client-integration.md](../../chatgpt-tools-cdn/docs/client-integration.md)

---

## 1. 架构边界

| 职责 | 位置 | 说明 |
|------|------|------|
| 换肤注入 | 本地引擎 / CDP | **不变**；云端只做分发 |
| Catalog / 公告 / 版本元数据 | CDN JSON | Rust 拉取并落盘 |
| `.cgskin` 下载 | **仅 Rust** | WebView **不得** 用任意 URL 下包 |
| 皮肤应用 | `list_skins` → materialize → apply | cache 与 user/bundled 同等可 apply |

```text
GUI (skinAPI)
  → Tauri commands (cloud_*)
  → src-tauri/src/cloud/*
       · host 白名单 + HTTPS（本机 http 仅 127.0.0.1/localhost）
       · catalog 驱动 package.url / mirrors
       · sha256 + 体积硬限 + import 级校验
       · 写入 %LOCALAPPDATA%\ChatGPTTools\cache\skins\<id>
  → list_skins 合并 user > cache > bundled
  → status 再 merge catalog 远程-only 条目（source=remote）
```

---

## 2. 开发预览 Base URL

默认（未配置时）：

```text
http://127.0.0.1:8788/v1
```

对应 CDN 仓：

```powershell
cd E:\demo\chatgpt-tools-cdn
npm install
npm run serve
# → http://127.0.0.1:8788/v1/health.json
```

### 配置方式

**环境变量（优先）**

| 变量 | 含义 |
|------|------|
| `CODEX_SKIN_CLOUD_URL` | 覆盖 baseUrl，如 `http://127.0.0.1:8788/v1` |
| `CODEX_SKIN_CLOUD_CHANNEL` | `stable` / `beta` |
| `CODEX_SKIN_CLOUD_DISABLED=1` | 关闭云端 |
| `CODEX_SKIN_APP_VERSION` | 覆盖版本过滤用的应用版本（默认与 GUI `2.2.0` 对齐） |

**settings.json**（`%LOCALAPPDATA%\ChatGPTTools\settings.json`）

```json
{
  "cloud": {
    "enabled": true,
    "baseUrl": "http://127.0.0.1:8788/v1",
    "channel": "stable",
    "timeoutMs": 15000,
    "allowedHosts": [
      "127.0.0.1",
      "localhost",
      "cdn.example.com",
      "github.com",
      "objects.githubusercontent.com",
      "r2.dev"
    ]
  }
}
```

---

## 3. 本地磁盘布局

```text
%LOCALAPPDATA%\ChatGPTTools\
  settings.json              # 可含 cloud 段
  skins\                     # 用户 import（最高优先级）
  cache\
    skins\<id>\              # 云端下载缓存（含 skin.json + .cache-meta.json）
    tmp\                     # 下载/解压临时区
  cloud\
    catalog.json
    catalog.etag
    announcements.json
    announcements.etag
    read-state.json          # { "readIds": ["..."] }
  runtime-skins\             # materialize 运行时副本
```

`.cache-meta.json` 示例：

```json
{
  "version": "2.0.0",
  "sha256": "abc…64hex",
  "downloadedAt": "…",
  "sourceUrl": "https://…",
  "size": 1234567,
  "channel": "stable",
  "catalogId": "cyberpunk"
}
```

**缓存命中**：同 `id` + `version` + `sha256` 时 `cloud_download_skin` 直接返回 `cached: true`，不重复下载。

---

## 4. 前端 API（`window.skinAPI`）

| 方法 | 后端命令 | 说明 |
|------|----------|------|
| `cloudStatus({ force })` | `cloud_status` | 快照：配置、disk catalog、公告、版本、已缓存 id |
| `cloudRefresh()` | `cloud_refresh` | 强制拉 catalog + announcements |
| `cloudAnnouncements({ refresh })` | `cloud_announcements` | 过滤后的公告列表 |
| `cloudMarkAnnouncementRead(id)` | `cloud_mark_announcement_read` | 写入已读 |
| `cloudDownloadSkin(skinId)` | `cloud_download_skin` | **仅 skinId**，URL 只来自 catalog |
| `cloudCheckUpdate()` | `cloud_check_update` | minAppVersion / 可选 version.json |
| `cloudClearSkinCache(skinId?)` | `cloud_clear_skin_cache` | 清单个或全部缓存 |

`status()` 在返回前会把 **磁盘上的 catalog** merge 进 `skins`（远程-only 卡片、`updateAvailable` 等）。网络刷新请调 `cloudRefresh()` 后再 `status()`（GUI「刷新」已串起来）。

---

## 5. GUI 行为

### 5.1 公告位 `#promoBanner`

- 启动：`cloudStatus` 读本地公告 → 绘制 banner  
- 后台：`cloudRefresh` 更新  
- 多条：轮播 + dots；关闭 → `cloudMarkAnnouncementRead`  
- 无网 / 无公告：回退本地默认文案  

### 5.2 皮肤卡片

| source / installState | 主按钮 | 标签 |
|----------------------|--------|------|
| bundled / user / cache + ready | 使用 / 重新应用 | 内置 / 已导入 / 已缓存 |
| remote | **下载皮肤** | 云端 |
| updateAvailable | **更新皮肤** | 可更新 |

侧栏分类：**不**由客户端关键词推断。本地 `skin.json.categories` 与 catalog 条目的 `categories`（字符串数组，如 `["anime","tech"]`）声明归属；侧栏按钮本身来自 `src/skin-categories.json`。本地非空 `categories` 优先于 catalog。

下载走 `cloudDownloadSkin(id)`，成功后列表出现 `source=cache`，可 apply。

### 5.3 关于页「检查更新」

调用 `cloudCheckUpdate()`：

1. 优先 `GET {baseUrl}/version.json`（云端管理后台「版本号」配置）  
2. 读取 `latest` / `minAppVersion` / `downloadUrl` / `message`  
3. 结果写入 `#aboutUpdateStatus`；有 `downloadUrl` 时可打开下载页  

### 5.4 公告位不显示标题

`#promoBanner` 只展示公告 **正文**（`body`）；管理端的 `title` 仅用于后台列表检索，不渲染到客户端。

---

## 6. 安全模型（必读）

目标：**任意前端 hook / 伪造 invoke 参数都不能变成「按 URL 下任意文件」**。

| 控制 | 实现 |
|------|------|
| 无任意 URL IPC | `cloud_download_skin` 只收 `skin_id` |
| Catalog 权威 | package.url / mirrors / sha256 / size 仅从 catalog 读取 |
| Host 白名单 | `allowedHosts`；子域与 `*.` 模式；每跳重定向重验 |
| 协议 | 非本机仅 HTTPS；本机开发允许 `http://127.0.0.1` / `localhost` |
| 完整性 | sha256 必为 64 hex；**全 0 占位符拒绝** |
| 体积 | `MAX_PACKAGE_BYTES`（48MB）+ 可选 size 精确匹配 |
| 内容 | 复用 `validate_skin_manifest`；去掉 inject；包内 id 须与 catalog 一致 |
| 原子安装 | staging 目录 rename；失败不留半截合法缓存 |
| WebView CSP | `connect-src` 不含任意 CDN 文件域；大包不在 JS 里 fetch |

---

## 7. 列表合并优先级

```text
同 id：user > cache > bundled
另：catalog 中本地没有且带 package 的 → source=remote（仅展示，不可 apply 直到下载）
```

`bundledWithApp: true` 且无 package 的条目不会作为远程卡片出现（依赖安装包内置）。

---

## 8. 联调清单

1. 启动 CDN：`chatgpt-tools-cdn` → `npm run serve`  
2. 确认 `http://127.0.0.1:8788/v1/health.json` 与 `.../stable/catalog.json`  
3. 本仓 `npm run dev`（可设 `CODEX_SKIN_CLOUD_URL=http://127.0.0.1:8788/v1`）  
4. 顶部公告应变为云端文案；关于页检查更新走云端  
5. catalog 示例 `cyberpunk` 使用占位 sha256 → **下载应失败**（安全正确）  
6. 发布真实皮肤：用 CDN `pack-skin` 生成 `.cgskin` + 真 sha256 + 可访问 URL 后再测下载与缓存二次命中  

---

## 9. 相关源码

| 路径 | 职责 |
|------|------|
| `src-tauri/src/cloud/` | 配置、HTTP、catalog、公告、下载、版本 |
| `src-tauri/src/commands.rs` | `cloud_*` 命令 |
| `src-tauri/src/cdp/native.rs` | `list_skins` 含 cache；删除 cache |
| `src/skin-api.js` | 前端桥 |
| `src/app.js` | banner / 下载按钮 / 检查更新 |
| `src-tauri/tauri.conf.json` | CSP（本机预览 connect） |

---

## 10. 离线行为

| 场景 | 行为 |
|------|------|
| 无网 | 用 `cloud/*.json` 磁盘缓存；公告可显示旧数据 |
| 已缓存皮肤 | 可 apply |
| 远程未下载 | 卡片仍可显示（若 catalog 有缓存）；下载失败提示 |
| 云端关闭 | `CODEX_SKIN_CLOUD_DISABLED=1` 或 `cloud.enabled: false` |
