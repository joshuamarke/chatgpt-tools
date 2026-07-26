import WebSocket from "ws";
const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find(p => p.type==="page") || pages[0];
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id=0; const pending=new Map();
const send=(method,params={})=>new Promise((res,rej)=>{const i=++id;pending.set(i,{res,rej});ws.send(JSON.stringify({id:i,method,params}));setTimeout(()=>rej(new Error("to")),20000);});
ws.on("message",d=>{const m=JSON.parse(d);if(m.id&&pending.has(m.id)){pending.get(m.id).res(m);pending.delete(m.id);}});
await new Promise(r=>ws.once("open",r));
await send("Runtime.enable");

const expr = `(() => {
  const root = getComputedStyle(document.documentElement);
  const keys = [...root].filter(k => /dropdown|menu|popover|bg-primary|main-surface|base-surface|background-surface|list-hover|editor-widget|side-bar/i.test(k));
  const vals = {};
  for (const k of keys.sort()) vals[k] = root.getPropertyValue(k).trim().slice(0,80);

  // Find CSS rules defining bg-token-main-surface-primary and dropdown
  const rules = [];
  for (const sheet of document.styleSheets) {
    let rs; try { rs = sheet.cssRules; } catch { continue; }
    if (!rs) continue;
    for (const r of rs) {
      const t = r.cssText || "";
      const sel = r.selectorText || "";
      if (/bg-token-main-surface-primary|bg-token-dropdown|token-dropdown-background|color-token-menu|color-token-dropdown|\.bg-token-bg-primary/i.test(sel) || (/bg-token-main-surface-primary|dropdown-background|menu-background/.test(t) && t.length < 500)) {
        rules.push(t.slice(0,350));
        if (rules.length >= 30) break;
      }
    }
    if (rules.length >= 30) break;
  }

  // Try click top bar / account / mode switch for menus
  const candidates = [
    ...document.querySelectorAll('button[aria-haspopup]'),
    ...document.querySelectorAll('[aria-haspopup="menu"]'),
    ...document.querySelectorAll('header button, [class*="application-menu"] button'),
  ].slice(0,20).map(b => ({
    label: b.getAttribute('aria-label') || b.innerText?.slice(0,40) || '',
    haspopup: b.getAttribute('aria-haspopup'),
    cls: String(b.className).slice(0,100)
  }));

  return { vals, rules, candidates, hasJiuyi: document.documentElement.classList.contains('codex-jiuyi-skin') };
})()`;
const r = await send("Runtime.evaluate",{expression:expr,returnByValue:true});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
