const expr = `(() => {
  // Click first suggestion if present to open list, wait not possible sync - just dump native structure more
  const sug = document.querySelector(".group\\\\/home-suggestions");
  const firstBtn = sug && sug.querySelector("button");
  const banner = firstBtn && firstBtn.closest(".relative");
  // dump parent hierarchy for suggestions
  let el = sug, chain=[];
  for (let i=0;i<8 && el;i++) {
    const cs=getComputedStyle(el);
    chain.push({
      tag: el.tagName,
      class: String(el.className||"").slice(0,220),
      pos: cs.position,
      top: cs.top,
      mt: cs.marginTop,
      align: cs.alignItems,
      justify: cs.justifyContent,
      flex: cs.flex,
      y: Math.round(el.getBoundingClientRect().top),
      h: Math.round(el.getBoundingClientRect().height)
    });
    el = el.parentElement;
  }
  // composer send button detail
  const send = document.querySelector(".composer-surface-chrome button.bg-token-foreground, .composer-surface-chrome button[class*='size-token-button-composer']");
  const sendCs = send ? getComputedStyle(send) : null;
  // native CSS vars used on suggestion inset
  const root = getComputedStyle(document.documentElement);
  const vars = {};
  for (const k of ["--composer-suggestion-inline-inset","--thread-content-max-width","--token-foreground","--main-surface-primary","--text-primary","--composer-background"]) {
    vars[k] = root.getPropertyValue(k).trim();
  }
  // also from main
  const main = document.querySelector("main");
  const mainVars = {};
  if (main) {
    const m = getComputedStyle(main);
    for (const k of ["--composer-suggestion-inline-inset","--thread-content-max-width"]) mainVars[k]=m.getPropertyValue(k).trim();
  }
  return { chain, send: send ? { class: String(send.className).slice(0,200), bg: sendCs.backgroundColor, color: sendCs.color, opacity: sendCs.opacity, html: send.outerHTML.slice(0,400) } : null, vars, mainVars, hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin") };
})()`;
import WebSocket from "ws";
const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find(p => p.type==="page") || pages[0];
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id=0; const pending=new Map();
const send = (method, params={}) => new Promise((res,rej)=>{ const i=++id; pending.set(i,{res,rej}); ws.send(JSON.stringify({id:i,method,params})); setTimeout(()=>rej(new Error("to")),12000);});
ws.on("message", d=>{ const m=JSON.parse(d); if(m.id&&pending.has(m.id)){pending.get(m.id).res(m); pending.delete(m.id);}});
await new Promise(r=>ws.once("open",r));
await send("Runtime.enable");
const r = await send("Runtime.evaluate",{expression:expr,returnByValue:true});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
