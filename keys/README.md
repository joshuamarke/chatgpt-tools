# 发版密钥与打包机密

下列内容与 **签名私钥** 同级：只存在于维护者本机 / CI Secrets，**禁止写入源码、Issue、提交记录**。

| 机密 | 存放位置 | 用途 |
|------|----------|------|
| Updater 签名私钥 | `keys/chatgpt-tools.key`（gitignore） | 签发安装包 `.sig` |
| 生产云端 baseUrl（可选） | `keys/release.env` 或 CI Secrets | 打包时嵌入 catalog CDN |
| Updater endpoints（可选覆盖） | 同上 | 默认自动用 **GitHub Releases `latest.json`** |

仓库可提交：

- `chatgpt-tools.key.pub`（公钥，已写入 `tauri.conf.json` → `plugins.updater.pubkey`）
- `release.env.example`（变量名 + 示例，无真实机密）
- 本 README

---

## 1. Updater 签名密钥（正式发版必需）

```bash
npx tauri signer generate -w keys/chatgpt-tools.key --ci
```

将打印的 **Public** 写入 `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`（或确认与 `keys/*.key.pub` 一致）。

### 本机签名构建

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = (Resolve-Path keys/chatgpt-tools.key).Path
# 可选：$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "..."

# 可选：keys/release.env；不配则 updater 默认走 GitHub latest.json（需已 stamp 仓库地址）
npm run build
```

### GitHub Actions Secrets（签名）

| Secret | 必需 | 值 |
|--------|------|-----|
| `TAURI_SIGNING_PRIVATE_KEY` | **正式 Release 是** | 私钥全文（与本机 `.key` 相同） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 否 | 生成密钥时若设了密码 |

丢失私钥后无法为现有公钥继续签发，只能换密钥并要求用户重装。

---

## 2. 更新检查地址（GitHub Releases，默认自动）

打包时 `scripts/inject-release-config.mjs` 会写入 updater `endpoints`：

1. 若设置了 `TAURI_UPDATER_ENDPOINTS`（Secret / `release.env`）→ 用你的列表  
2. 否则 → **自动**  
   `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`

`<owner>/<repo>` 来源：

| 环境 | 来源 |
|------|------|
| GitHub Actions | `GITHUB_REPOSITORY`（自动，无需 Secret） |
| 本机 | `src/repo-meta.json` 的 `owner` + `name`，或 `REPO_OWNER` / `REPO_NAME` |

创建仓库后任选其一：

本仓库已配置为 **`joshuamarke/chatgpt-tools`**（见 `src/repo-meta.json`）。

默认 updater：

```text
https://github.com/joshuamarke/chatgpt-tools/releases/latest/download/latest.json
```

Release workflow 在上传安装包后会生成并挂上 **`latest.json`**，与上述 URL 对齐。

### 可选：云端 catalog

| Secret / 变量 | 正式发版 |
|---------------|----------|
| `CODEX_SKIN_CLOUD_URL` | **否**（仅当你有独立皮肤 CDN 时） |
| `CODEX_SKIN_CLOUD_EXTRA_HOSTS` | 否 |
| `TAURI_UPDATER_ENDPOINTS` | **否**（默认 GitHub；有 CDN 镜像时再覆盖） |

若必须强制云端：构建时设 `REQUIRE_CLOUD_URL=1`。

---

## 3. 创建 GitHub 仓库后的检查清单

1. 仓库已创建：https://github.com/joshuamarke/chatgpt-tools  
2. 推送代码（含 `.github/workflows/` 与已 stamp 的 `src/repo-meta.json`）。  
3. **Settings → Secrets and variables → Actions** 添加：  
   - `TAURI_SIGNING_PRIVATE_KEY`（必需，正式发版）  
   - 可选：`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`、`CODEX_SKIN_CLOUD_URL`、`TAURI_UPDATER_ENDPOINTS`  
4. 发版：升版本 → 打 tag → GitHub **Publish Release** → 等待 **Release assets** workflow。  
5. 装包后「检查更新」应能读到该 Release 上的 `latest.json`。

### 开发

`npm run dev` **不**注入生产 updater/云端；未嵌入时云端默认 `http://127.0.0.1:8788/v1`。

```powershell
$env:CODEX_SKIN_CLOUD_URL = "http://127.0.0.1:8788/v1"
npm run dev
```

---

## 4. 发版后运维（可选镜像）

可将 Release 上的 `latest.json` 同步到自建 CDN，并把 CDN URL 写在 `TAURI_UPDATER_ENDPOINTS` **最前面**，GitHub 地址作 fallback。真实 CDN 域名只放 Secrets / `release.env`。
