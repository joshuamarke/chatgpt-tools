import WebSocket from "ws";
const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find(p => p.type==="page") || pages[0];
if (!page?.webSocketDebuggerUrl) { console.log("no cdp"); process.exit(0); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id=0; const pending=new Map();
const send=(method,params={})=>new Promise((res,rej)=>{const i=++id;pending.set(i,{res,rej});ws.send(JSON.stringify({id:i,method,params}));setTimeout(()=>rej(new Error("to")),15000);});
ws.on("message",d=>{const m=JSON.parse(d);if(m.id&&pending.has(m.id)){pending.get(m.id).res(m);pending.delete(m.id);}});
await new Promise(r=>ws.once("open",r));
await send("Runtime.enable");
const expr = `(() => {
  const pick = (el) => {
    if (!el) return null;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      tag: el.tagName,
      class: String(el.className||"").slice(0,280),
      role: el.getAttribute("role"),
      bg: cs.backgroundColor,
      bgImg: cs.backgroundImage?.slice(0,120),
      color: cs.color,
      border: cs.borderColor,
      w: Math.round(r.width),
      h: Math.round(r.height),
      y: Math.round(r.top),
      x: Math.round(r.left),
    };
  };
  const fade = document.querySelector("div.pointer-events-none.absolute.inset-x-0.bottom-0 > div[class*='bg-gradient']")
    || document.querySelector("div.pointer-events-none.absolute.inset-x-0.bottom-0 > div.mx-auto");
  const composer = document.querySelector(".composer-surface-chrome");
  const sug = document.querySelector(".group\\\\/home-suggestions") || document.querySelector('[class*="home-suggestions"]');
  const listItems = [...document.querySelectorAll('[class*="home-suggestion-list-item"], [class*="suggestion-list"]')].slice(0,8).map(pick);
  // also buttons that open lists
  const sugBtns = sug ? [...sug.querySelectorAll("button")].slice(0,4).map(pick) : [];
  // any open menus
  const menus = [...document.querySelectorAll('[role="menu"],[role="listbox"],[data-radix-popper-content-wrapper], [data-state="open"]')].slice(0,15).map(el => ({
    ...pick(el),
    text: (el.innerText||"").replace(/\\s+/g," ").slice(0,100),
    parentBg: el.parentElement ? getComputedStyle(el.parentElement).backgroundColor : null,
  }));
  // root tokens
  const root = getComputedStyle(document.documentElement);
  const tokens = {};
  for (const k of ["--main-surface-primary","--main-surface-secondary","--surface-primary","--text-primary","--composer-background","--token-main-surface-primary","--bg-primary","--dropdown-background"]) {
    tokens[k] = root.getPropertyValue(k).trim() || root.getPropertyValue(k.replace("--","--token-")).trim();
  }
  // find CSS variables actually used by host for dropdowns
  const sample = document.querySelector('[class*="dropdown"], [class*="token-dropdown"], [class*="bg-token"]');
  return {
    hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin"),
    rootClass: document.documentElement.className,
    fade: pick(fade),
    fadeParent: fade?.parentElement ? pick(fade.parentElement) : null,
    composer: pick(composer),
    sug: pick(sug),
    sugSectionClass: sug ? String(sug.className).slice(0,300) : null,
    sugInnerHTML: sug ? sug.innerHTML.slice(0,500) : null,
    listItems,
    sugBtns,
    menus,
    tokens,
  };
})()`;
const r = await send("Runtime.evaluate",{expression:expr,returnByValue:true});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
