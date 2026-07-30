# GUI 原语约定

管理端（非皮肤宿主）界面样式统一约定。新增 UI 时优先复用，避免再写平行自定义类。

## 按钮

| 场景 | 类名 |
|------|------|
| 工具栏 / 列表 / 卡片操作 | `chip-btn`，主操作 `chip-primary`，危险 `chip-danger`，警告 `chip-warn` |
| 弹窗确认 / 表单提交 | `button.primary` / `ghost` / `danger`，小尺寸加 `sm` |
| 28px 图标钮 | `icon-btn`，删除态加 `delete-btn` |

## 表单

| 场景 | 类名 |
|------|------|
| 字段栈（标签 + 控件 + 提示） | `prov-field` / `prov-field-label` / `prov-field-hint` / `prov-field-row` |
| 输入 + 旁侧按钮 | `prov-input-with-action` + `ghost sm prov-action-btn` |
| 分区 | `prov-form-section` / `prov-form-section-title` |
| 工具栏紧凑筛选 | `session-filter-input` / `session-filter-select` |
| 复选框标签 | `ui-check`（兼容 `check`、`session-filter-check`） |
| 行内探测结果 | `prov-probe-status` + `is-ok` / `is-warn` / `is-error` / `is-loading` |

## 布局与状态

| 场景 | 类名 |
|------|------|
| 列表空状态 | `session-empty` + `session-empty-title` / `session-empty-detail` |
| 嵌套空文案 | `ui-empty-inline` |
| 页头 | `view-header` / `view-title` / `view-lead`（兼容 `sessions-header`、`overview-header`） |
| 弹窗底栏 | `confirm-actions`；右对齐加 `is-end` 或 `form-actions` / `update-actions` |
| 状态语义色 | CSS 变量 `--status-ok-*` / `--status-warn-*` / `--status-error-*` / `--status-accent-*` / `--status-muted-*` |

## 圆角

- **状态徽章**（`*-badge`、`about-ver-badge`、计数 pill 等）：可用 `border-radius: 999px`
- **其余控件 / 卡片 / 弹窗**：最大 `12px`，优先 `8` / `10` / `12`（token：`--radius-sm` / `--radius-md` / `--radius-lg`）
- **圆形装饰**（spinner、圆点）：可用 `50%`

## 明确不合并

领域布局保持独立：`session-row`、`prov-card`、`ov-card`、皮肤 `.card`、`.about-hero*`、`.banner`、`.ui-select*`、`.prov-route-switch`、`.prov-live-bar` 结构等。

## 建议回归清单

样式或表单结构改动后，按下列表面过一遍（静态脚本可先跑 `npm run check:gui` 或 `node scripts/check-gui-regression.mjs`）：

| 表面 | 关注点 |
|------|--------|
| **概览** | 页头 title/lead、环境卡片 badge、安装 `chip-primary` |
| **皮肤** | 网格空状态、工具栏 ghost、自定义皮肤弹窗字段与底栏 |
| **会话** | Tab、工具行 provider 下拉（toolbar 密度）、筛选复选、列表空状态、删除确认 danger |
| **供应商列表** | live 条、保留登录 ui-check、卡片 badge/chip 操作 |
| **路由设置** | 端口旁「检测端口」、probe 状态、最大重试旁简洁 ui-check（日志/自动 FO 默认勾选）、队列行「切换」+ 上下移/移除、保存底栏 |
| **请求日志** | Tab 旁「日志」chip、加宽 `prov-logs-card`、保留天数 `prov-field`、筛选 `session-filter-*`、状态 `prov-badge`、详情 `ov-meta-list` |
| **编辑表单** | 分区字段、密码眼 icon-btn、连通/拉取模型、目录删除钮 |
| **关于** | hero、`about-ver-badge`（999 圆角）、更新状态 |
| **确认框** | primary / danger 实心删除、Esc/遮罩关闭 |
| **更新弹窗** | 底栏 ghost + primary 右对齐 |

### 圆角抽查

- 徽章 / 计数 pill：`999px` 仍在
- 弹窗卡片、按钮、输入框：`8` / `10` / `12`，无 `14+`
