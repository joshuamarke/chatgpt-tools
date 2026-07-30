# scripts/

仓库只保留**构建、发版、契约与冒烟**脚本。本地 CDP 探针 / 皮肤一次性补丁请放在 `scripts/local/`（已 gitignore）。

| 脚本 | 用途 | 入口 |
|------|------|------|
| `stamp-repo-meta.mjs` | 写入 `src/repo-meta.json`（GitHub 链接 / 默认 updater URL） | `npm run stamp:repo`；CI 自动 |
| `inject-release-config.mjs` | 打包注入 updater endpoints（默认 GitHub `latest.json`） | `npm run build` / `inject:release` |
| `stage-bundle-skins.mjs` | 安装包仅内置默认皮肤 | `beforeBuildCommand` / `stage:skins` |
| `ensure-resources.mjs` | 安装后资源完整性提示 | `postinstall` |
| `build-latest-json.mjs` | 从 Release 资产生成 Tauri `latest.json` | Release workflow |
| `verify-engine.mjs` | 引擎 CLI 冒烟 | `npm run verify:engine` |
| `doctor-selectors.mjs` | 宿主选择器契约静态检查 | `npm run doctor:selectors` |
| `check-gui-regression.mjs` | GUI 结构静态回归 | `npm run check:gui` |
| `macos/install-debug-launch-agent.sh` | macOS 调试端口 launchd（可选） | 见 `docs/development/macos-launch.md` |

引擎自检：`npm run test:engine` → `node engine/injector.mjs --self-test`。
