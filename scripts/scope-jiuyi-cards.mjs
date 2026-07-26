import fs from "fs";

const path = "E:/demo/chatgpt-tools/skins/jiuyi/assets/jiuyi-skin.css";
let css = fs.readFileSync(path, "utf8");

const card = `.jiuyi-home .group\\/home-suggestions button:not([class*="home-suggestion-list-item"])`;
const bare = `.jiuyi-home .group\\/home-suggestions button`;

// Only rewrite bare card selectors that are not already scoped
const patterns = [
  // longest first to avoid partial rewrites
  `${bare} > span:first-child > span:first-child,`,
  `${bare} span:has(> span > svg) {`,
  `${bare} span:has(> svg),`,
  `${bare} > span:last-child {`,
  `${bare} > span {`,
  `${bare}::after {`,
  `${bare}:hover {`,
  `${bare} svg {`,
  `${bare} {`,
];

const mapped = {
  [`${bare} > span:first-child > span:first-child,`]: `${card} > span:first-child > span:first-child,`,
  [`${bare} span:has(> span > svg) {`]: `${card} span:has(> span > svg) {`,
  [`${bare} span:has(> svg),`]: `${card} span:has(> svg),`,
  [`${bare} > span:last-child {`]: `${card} > span:last-child {`,
  [`${bare} > span {`]: `${card} > span {`,
  [`${bare}::after {`]: `${card}::after {`,
  [`${bare}:hover {`]: `${card}:hover {`,
  [`${bare} svg {`]: `${card} svg {`,
  [`${bare} {`]: `${card} {`,
};

let count = 0;
for (const p of patterns) {
  if (!css.includes(p)) {
    // maybe already scoped
    const alt = p.replace(bare, card);
    if (css.includes(alt)) {
      console.log("already:", p.slice(0, 60));
      continue;
    }
    console.error("missing:", JSON.stringify(p));
    process.exit(1);
  }
  // count occurrences of bare that aren't already :not
  const parts = css.split(p);
  // avoid double-scoping: if previous char sequence already has :not, skip
  css = parts.join(mapped[p]);
  count += parts.length - 1;
  console.log("scoped", count, p.slice(0, 70));
}

// Safety: any remaining bare (without :not) that still targets list?
const lines = css.split("\n");
const leftover = [];
for (let i = 0; i < lines.length; i++) {
  if (
    lines[i].includes(".group\\/home-suggestions button") &&
    !lines[i].includes(":not([class*=\"home-suggestion-list-item\"])")
  ) {
    leftover.push(`${i + 1}: ${lines[i].trim()}`);
  }
}
console.log("leftover unscoped:", leftover.length);
leftover.forEach((l) => console.log(" ", l));

if ((css.match(/data:image\/png;base64,/g) || []).length < 1) {
  console.error("lost data url");
  process.exit(1);
}

fs.writeFileSync(path, css, "utf8");
console.log("Wrote", Buffer.byteLength(css, "utf8"));
