import WebSocket from "ws";
const pages = await (await fetch("http://127.0.0.1:9335/json")).json();
const page = pages.find(p => p.type==="page") || pages[0];
if (!page?.webSocketDebuggerUrl) { console.log("no cdp"); process.exit(0); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id=0; const pending=new Map();
const send=(method,params={})=>new Promise((res,rej)=>{const i=++id;pending.set(i,{res,rej});ws.send(JSON.stringify({id:i,method,params}));setTimeout(()=>rej(new Error("to")),12000);});
ws.on("message",d=>{const m=JSON.parse(d);if(m.id&&pending.has(m.id)){pending.get(m.id).res(m);pending.delete(m.id);}});
await new Promise(r=>ws.once("open",r));
await send("Runtime.enable");
const expr = `(() => {
  const nodes = [...document.querySelectorAll("div.pointer-events-none.absolute.inset-x-0.bottom-0")];
  const hits = nodes.map(el => {
    const inner = el.querySelector("div.mx-auto, div[class*='bg-gradient'], div[class*='max-w']");
    const cs = inner ? getComputedStyle(inner) : null;
    const outerCs = getComputedStyle(el);
    return {
      outerClass: String(el.className).slice(0,300),
      outerBg: outerCs.backgroundImage + " | " + outerCs.backgroundColor,
      innerClass: inner ? String(inner.className).slice(0,350) : null,
      innerBgImage: cs?.backgroundImage,
      innerBgColor: cs?.backgroundColor,
      innerBgSize: cs?.backgroundSize,
      innerBgPos: cs?.backgroundPosition,
      rect: inner ? (() => { const r=inner.getBoundingClientRect(); return {y:Math.round(r.top),h:Math.round(r.height),w:Math.round(r.width)}; })() : null,
    };
  });
  // also broader search
  const grads = [...document.querySelectorAll("[class*='bg-gradient-to-t']")].slice(0,8).map(el => ({
    class: String(el.className).slice(0,280),
    bgImage: getComputedStyle(el).backgroundImage,
    bgColor: getComputedStyle(el).backgroundColor,
    parent: el.parentElement ? String(el.parentElement.className).slice(0,160) : null,
  }));
  return { hits, grads, hasJiuyi: document.documentElement.classList.contains("codex-jiuyi-skin") };
})()`;
const r = await send("Runtime.evaluate",{expression:expr,returnByValue:true});
console.log(JSON.stringify(r.result?.result?.value ?? r.result, null, 2));
ws.close();
