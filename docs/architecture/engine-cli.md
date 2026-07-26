# Engine CLI 协议

入口：`engine/cli.mjs`

```bash
node engine/cli.mjs <command> [options]
```

- 成功：stdout 一行 JSON，`exit 0`  
- 失败：stderr 一行 JSON `{ "ok": false, "error": "...", "code": "ENGINE_ERROR" }`，`exit 1`  

环境变量：

| 变量 | 说明 |
|------|------|
| `CODEX_SKIN_ROOT` | 资源根（含 `engine/`、`skins/`） |
| `CODEX_SKIN_STATE_NAME` | 状态目录名，默认 `ChatGPTTools` |
| `CODEX_SKIN_PORT` | CDP 端口，默认 `9335` |
| `CODEX_APP_PATH` | 强制指定客户端路径 |
| `CODEX_CONFIG_PATH` | 覆盖 `~/.codex/config.toml` |
| `CODEX_SKIN_NODE` | （Rust 侧）指定 node 可执行文件 |
| `CODEX_SKIN_SLOW_SCALE` | 超时倍率 `1`–`3`，慢机器可设 `2` |
| `CODEX_SKIN_NATIVE` | `0`/`false` 关闭 Rust 原生 CDP 路径（默认开启） |
| `CODEX_SKIN_FORCE_NODE` | `1`/`true` 强制所有引擎命令走 Node CLI |

### Tauri 原生路径（Rust CDP）

前端仍调用同一组 Tauri commands；`engine::run_engine` 在进程内优先：

| 命令 | 原生条件 | 说明 |
|------|----------|------|
| `version` / `paths` | 始终 | 无 Node |
| `detect` / `status` | 始终（失败则回退） | 本机 FS + loopback CDP HTTP；detect 含 exe 快速探测 |
| `resolve-asset` | 始终（失败则回退） | 读 `skin.json` + 磁盘路径 |
| `set-app-path` / `clear-app-path` | 始终 | 写 `%LOCALAPPDATA%\ChatGPTTools\settings.json` |
| `delete-skin` | 始终 | 删用户皮肤 / 用户覆盖目录 |
| `restore` | 始终 | CDP remove（若就绪）+ 剥离 `config.toml` [desktop] appearance* + 清 state；**不**自动软重启客户端 |
| `apply` | 始终（失败可回退 Node） | 进程内 `ensure_debug_port` + staged inject；成功后 `keep` 后台 re-inject 保持刷新 |
| `export-skin` / `import-skin` / `inspect-skin` | 始终（失败可回退） | Rust `zip` crate，无 adm-zip |
| `design-wallpaper` 等 | — | 仍可 `node engine/cli.mjs` |

主路径目标：日常换肤与皮肤包 **无需系统 Node**；`keep` 用进程内轮询 soft-verify，替代 Node `injector.mjs --watch`。

---

## 命令

### `version`

```json
{
  "ok": true,
  "name": "chatgpt-tools-engine",
  "version": "2.2.0",
  "protocol": 2,
  "root": "..."
}
```

版本号以 `engine/version.js` 为准。

### `paths`

```json
{ "ok": true, "root": "...", "stateRoot": "...", "bundledSkins": "...", "userSkins": "..." }
```

### `status`

含：

| 字段 | 说明 |
|------|------|
| `platform` / `configPath` / `stateRoot` / `state` | 路径与会话 |
| `lifecycle` | `offline` \| `starting` \| `ready` |
| `processRunning` | OS 进程探测（多策略，允许假阴性） |
| `debugPortOpen` | CDP `/json/version` 可通 |
| `debugReady` / `rendererReady` | 存在 `app://` 可注入页 |
| `codexRunning` | **进程 ∪ 端口 ∪ 渲染页**（慢启动不应误报未打开） |
| `paused` / `protocol` / `engineVersion` | 控制与版本 |
| `shellOk` / `artOk` / `artPending` | 最近一次 apply 结果 |
| `injectorAlive` | watch 进程是否仍为我们的 injector |
| `skins[]` | `id, name, …, active, previewUrl, appearance, …` |

### `detect`

客户端探测结果。

### `apply --skin-id <id> [--restart true|false]`

互斥锁保护。成功后 state 含 `browserId`、`injectorScript`、`nodePath`、`shellOk`、`artOk`、`artPending`。  
快速路径：injector **`--soft` once**（shell 通过即成功；大立绘可 `artPending`）。  
同皮肤 + 存活 watch 时走热路径，尽量不 stop/spawn。

### `restore [--restore-theme true|false]`

停 injector → purge-all → 可选还原 config → 软重启应用。  
`wasRunning` 使用 lifecycle，不只依赖进程列表。

### `pause` / `resume [--restart true|false]`

- `pause`：写 `paused.flag` 并 CDP remove 当前皮肤  
- `resume`：清暂停并重新 `apply` 状态中的皮肤  

### `verify --skin-id <id>`

**Hard verify**（非 soft）：主壳 + style；侧栏可选。

### `check-payload --skin-id <id>`

不连 CDP：构建 staged payload 并返回体积 / fingerprint / art 元数据。  
`recommended: false` 仅表示超过软提示阈值，**不是错误**（允许高质量原图）。

### `export-skin` / `import-skin` / `inspect-skin` / `delete-skin`

多皮肤包能力；`inspect` 扫描 inject+plugin 风险与立绘体积。

### `design-wallpaper --payload <json-file|json-string>`

基于 **目标皮肤模板**（`baseSkinId`）生成用户自定义皮肤：复制模板资源，替换壁纸，合并 `art.*` / 颜色 tokens，并追加不破坏模板布局的 designer CSS。

壁纸硬上限 **16 MB**（与 `MAX_ART_BYTES` 一致）。

Payload 字段（camelCase）：

```json
{
  "baseSkinId": "dream",
  "imagePath": "C:/path/to.jpg",
  "name": "我的自定义皮肤",
  "fit": "cover",
  "position": "right center",
  "accent": "#8b7cff",
  "background": "#f7f8fc",
  "text": "#202536",
  "panel": "#ffffff",
  "font": "system",
  "radius": 16,
  "overlay": 12,
  "opacity": 92,
  "appearance": "auto",
  "focusX": 0.72,
  "focusY": 0.45,
  "safeArea": "auto",
  "taskMode": "auto"
}
```

### `set-app-path` / `clear-app-path` / `resolve-asset` / `list-skins`

同前。

---

## 协议版本

- `protocol: 1`：历史  
- **`protocol: 2`**：共享 runtime、pause/resume、check-payload、status.paused、载荷限制  
- **引擎 2.2**：lifecycle 字段、shellOk/artOk、慢启动探测（向后兼容，字段只增不删）

新增字段应向后兼容；破坏性变更需 bump `protocol` 并同步 Rust `commands.rs` 与 `skin-api.js`。

## 新增命令检查表

1. `cli.mjs` `dispatch`  
2. 若 GUI 需要：`commands.rs` + `lib.rs` + `skin-api.js`  
3. 更新本文档与 README  
4. `npm run test:engine` / `verify:engine`  

## 底层 injector

```bash
node engine/injector.mjs --watch|--once|--verify|--remove|--check-payload|--self-test \
  --port 9335 --skin-dir <dir> [--soft] [--browser-id <id>] [--pause-file <path>]
```

## 宿主生命周期（host-probe）

```text
processRunning ──┐
debugPortOpen  ──┼──► codexRunning / lifecycle
rendererReady  ──┘
                     offline  = 全无
                     starting = 有进程或端口，尚无 app://
                     ready    = 可注入
```

慢机器上 **禁止** 仅用 `Get-Process` 判定「未打开」。
