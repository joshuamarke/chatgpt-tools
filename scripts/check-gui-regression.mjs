/**
 * Static GUI regression checks after style unification.
 * Run: node scripts/check-gui-regression.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => fs.readFileSync(path.join(root, p), "utf8");

const html = read("src/index.html");
const css = read("src/styles.css");
const uiSelect = read("src/ui/ui-select.js");
const provView = read("src/features/providers/providers-view.js");
const appJs = read("src/app.js");

/** @type {{ name: string, ok: boolean, detail?: string }[]} */
const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok: !!ok, detail });
};

// —— 概览 / 页头 ——
check("概览页头 overview-header", /id="overviewView"[\s\S]*?class="overview-header"/.test(html));
check("概览刷新 ghost sm", /id="btnEnvRefresh"[\s\S]*?class="ghost sm"|class="ghost sm" id="btnEnvRefresh"/.test(html));
check("页头别名 view-header CSS", /\.view-header[\s\S]*?\.overview-header[\s\S]*?\.sessions-header/.test(css));

// —— 皮肤 ——
check("皮肤空状态 empty-state", /empty-state/.test(appJs) && /\.empty-state\s*\{/.test(css));
check("自定义皮肤 chip-primary", /id="btnWallpaper"[\s\S]*?chip-primary|chip-primary[\s\S]*?btnWallpaper/.test(html));
check("壁纸表单 prov-field", /id="wallpaperForm"[\s\S]*?class="prov-field"/.test(html));
check("壁纸底栏 form-actions+confirm-actions", /form-actions confirm-actions is-end/.test(html));
check("壁纸滑条 range-label", /prov-field range-label/.test(html) && /\.range-label\s*>\s*\.prov-field-label/.test(css));

// —— 会话 ——
check("会话空状态 session-empty", /id="sessEmpty"[\s\S]*?session-empty-title/.test(html));
check("会话工具字段 prov-field sess-field", /prov-field sess-field/.test(html));
check("会话 select toolbar variant", /data-ui-select-variant="toolbar"/.test(html));
check("ui-select 识别 toolbar", /data-ui-select-variant/.test(uiSelect) && /ui-select--toolbar/.test(uiSelect));
check("会话复选 ui-check", /sessGroupByProject[\s\S]*?ui-check|ui-check[\s\S]*?sessGroupByProject/.test(html));

// —— 供应商 ——
check("供应商空状态复用 session-empty", /id="provEmpty"[\s\S]*?session-empty-title/.test(html));
check("路由弹窗 input-with-action 端口旁检测", /id="provRouteListenPort"[\s\S]{0,280}btnProvRouteCheckPort/.test(html));
check("路由选项 ui-check", /id="provRouteLogging"/.test(html) && /ui-check session-filter-check/.test(html));
check("路由/表单底栏 provider-form-actions", /confirm-actions provider-form-actions/.test(html));
check("密码眼 icon-btn", /icon-btn prov-password-toggle/.test(html));
check("目录删除 icon-btn delete-btn", /icon-btn delete-btn prov-catalog-del/.test(provView));
check("端口状态 probe-status", /id="provRoutePortStatus"[\s\S]*?prov-probe-status|class="prov-probe-status" id="provRoutePortStatus"/.test(html));

// —— 关于 ——
check("关于版本徽章 about-ver-badge", /about-ver-badge/.test(html) && /\.about-ver-badge[\s\S]*?999px/.test(css));

// —— 确认框 / 更新 ——
check("确认框 confirm-actions", /id="confirmModal"[\s\S]*?confirm-actions/.test(html));
check("确认 danger 实心样式", /\.confirm-modal\s+\.confirm-actions\s+button\.danger/.test(css));
check("showConfirm 切换 danger/primary", /btnOk\.className\s*=\s*isDanger\s*\?\s*"danger"\s*:\s*"primary"/.test(appJs));
check("更新弹窗底栏", /update-actions confirm-actions is-end/.test(html));

// —— 全局约束 ——
const chipPrimaryDefs = (css.match(/\.chip-btn\.chip-primary\s*\{/g) || []).length;
check("chip-primary 仅一处定义", chipPrimaryDefs === 1, `count=${chipPrimaryDefs}`);
check("status token 存在", /--status-ok-fg/.test(css) && /--status-error-fg/.test(css));
const largeRadius = (css.match(/border-radius:\s*(?:1[3-9]|[2-9]\d)px/g) || []).length;
check("非徽章圆角 ≤12px", largeRadius === 0, `large=${largeRadius}`);
const pillRadius = (css.match(/border-radius:\s*999px/g) || []).length;
check("状态徽章 999px 保留", pillRadius >= 4, `count=${pillRadius}`);
check("ui-check 规范类", /\.ui-check\s*,/.test(css) || /\.ui-check\s*\{/.test(css));

const failed = results.filter((r) => !r.ok);
for (const r of results) {
  const mark = r.ok ? "OK  " : "FAIL";
  console.log(`${mark}  ${r.name}${r.detail ? ` (${r.detail})` : ""}`);
}
console.log("");
console.log(`Total ${results.length}, failed ${failed.length}`);
if (failed.length) {
  console.error("GUI regression checks failed.");
  process.exit(1);
}
console.log("GUI regression checks passed.");
