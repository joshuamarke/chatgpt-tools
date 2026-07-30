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
| `.skin` 下载 | **仅 Rust** | WebView **不得** 用任意 URL 下包 |
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

## 2. Base URL 与机密（重要）

**生产云端 baseUrl、updater endpoints、CDN 域名不得出现在源码或文档中。**  
与签名私钥同级：仅在 **打包阶段** 注入。

| 时机 | 来源 | 结果 |
|------|------|------|
| **正式 `npm run build`** | CI Secrets 或 `keys/release.env`（gitignore）→ `scripts/inject-release-config.mjs` | 写入 `src-tauri/gen/*`（gitignore），由 `build.rs` / Tauri overlay 嵌入安装包 |
| **开发 `npm run dev`** | 未注入时默认本机预览 | `http://127.0.0.1:8788/v1` |
| **运行时覆盖** | 环境变量 / `settings.json` | 仅本机调试，勿把生产域名写进仓库 |

安装包默认只内置 `qingkong`；其余皮肤从 **打包时写入的** catalog base 拉取，缓存到  
`%LOCALAPPDATA%\ChatGPTTools\cache\skins\<id>`。

**本地 CDN 预览**（开发）：

```powershell
cd E:\demo\chatgpt-tools-cdn
npm install
npm run serve
# → http://127.0.0.1:8788/v1/health.json

cd E:\demo\chatgpt-tools
$env:CODEX_SKIN_CLOUD_URL = "http://127.0.0.1:8788/v1"
npm run dev
```

变量名与示例文件：[`keys/release.env.example`](../keys/release.env.example)、[`keys/README.md`](../keys/README.md)。

### 配置方式

**环境变量**

| 变量 | 含义 |
|------|------|
| `CODEX_SKIN_CLOUD_URL` | 运行时覆盖 baseUrl；**打包时**亦由此（或 `keys/release.env`）注入生产地址 |
| `TAURI_UPDATER_ENDPOINTS` | **仅打包**：`latest.json` 多 endpoint（逗号或 JSON 数组） |
| `CODEX_SKIN_CLOUD_EXTRA_HOSTS` | **仅打包**：额外下载白名单 host |
| `REQUIRE_RELEASE_SECRETS=1` | 正式发版：缺少云端/updater 机密则构建失败 |
| `CODEX_SKIN_CLOUD_CHANNEL` | `stable` / `beta` |
| `CODEX_SKIN_CLOUD_DISABLED=1` | 关闭云端 |
| `CODEX_SKIN_APP_VERSION` | 覆盖版本过滤用的应用版本（默认与 GUI 对齐） |
| `CODEX_SKIN_BUNDLE_SKINS` | 仅影响 `tauri build`：逗号分隔内置皮肤 id（默认 `qingkong`） |

**settings.json**（`%LOCALAPPDATA%\ChatGPTTools\settings.json`，本机调试示例）

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
      "github.com",
      "objects.githubusercontent.com",
      "r2.dev"
    ]
  }
}
```

生产 host 由打包注入的 allowlist 自动带上；不要在文档或示例里写真实 CDN 域名。

---

## 3. 本地磁盘布局

```text
%LOCALAPPDATA%\ChatGPTTools\
  settings.json              # 可含 cloud 段
  skins\                     # 用户 import（最高优先级）
  cache\
    skins\<id>\              # 云端下载缓存（含 skin.json + .cache-meta.json）
    previews\<id>\           # catalog 缩略图缓存（image.* + meta.json；未下完整包也可预览）
    tmp\                     # 下载/解压临时区
  cloud\
    catalog.json
    catalog.etag
    announcements.json
    announcements.etag
    about.json               # 联系信息（可含 html/css）
    about.etag
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
| `cloudEnsurePreviews(skinIds?)` | `cloud_ensure_previews` | 拉取/缓存 catalog `preview.url` 缩略图 → data-URL（列表渐进填充） |
| `cloudCheckUpdate()` | `cloud_check_update` | **已弃用（应用更新）**；皮肤 catalog 版本过滤仍可能用到 |
| `cloudAbout({ refresh })` | `cloud_about` | 关于页联系信息（`/about.json`，与 version 分离） |
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

#### 预览图（未下载完整包也能看）

CSP 禁止 WebView 直连远程 `img`，因此：

1. `status()` merge catalog 后，把磁盘 `cache/previews/<id>/` 填进 `previewUrl`（data-URL，无网络）
2. 列表绘制后 GUI 调 `cloudEnsurePreviews(missingIds)`：按 catalog `preview.url`（+ mirrors）白名单下载缩略图并落盘
3. 返回的 data-URL **渐进写入**卡片 DOM（不整表重建），避免只显示空渐变
4. 每轮网络拉取有预算（默认最多 6 张）；超出进 `pending`，GUI 延迟续拉；失败 id 有冷却，避免打爆 CDN heavy 限流
5. 完整 `.skin` 包仍只走 `cloudDownloadSkin`；清包缓存默认**保留**预览图

### 5.3 关于页「检查更新」（应用本体）

应用更新使用 **Tauri 官方 `tauri-plugin-updater`**，**不再**用皮肤 CDN 的 `version.json`：

1. 前端 `skinAPI.checkAppUpdate()` → `plugin:updater|check`  
2. 按 **打包时注入** 的 `plugins.updater.endpoints` 依次请求 `latest.json`（自动 fallback）  
3. 验签公钥：`plugins.updater.pubkey`（minisign，可提交）  
4. 用户确认后 `installAppUpdate` → 下载安装包 → `relaunchApp` 重启  

`latest.json` 由 GitHub Actions 在 Release 构建后挂到 Release；运维侧可再镜像到你在 `TAURI_UPDATER_ENDPOINTS` 里配置的优先地址（真实 URL 只放 Secrets / `keys/release.env`）。

皮肤包更新仍走 catalog / `cloudDownloadSkin`，与应用更新无关。

### 5.3.1 关于页「联系我们」

调用 `cloudAbout()`（读磁盘 / 可选网络刷新 `about.json`）：

1. 若 `contact.imageUrl` 非空 → 在 `#aboutContactImage` 显示联系介绍图  
2. 若 `contact.html` 非空 → 在 `#aboutContactRemote` 框架内渲染 HTML，并注入 `contact.css`  
3. 否则用 `email` / `website` / `note` 结构化兜底（`#aboutContactFallback`）  
4. 外链 `data-external`、邮箱 `data-mailto` 由关于页统一接管  

与「检查更新」**完全分离**，互不影响。

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
6. 发布真实皮肤：用 CDN `pack-skin` 生成 `.skin` + 真 sha256 + 可访问 URL 后再测下载与缓存二次命中  

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
