// 一次性 codemod：把写死的玻璃模糊/饱和接到全局旋钮上。
//
//   backdrop-filter: blur(16px) saturate(1.25);
//   → backdrop-filter: blur(calc(16px * var(--glass-blur-scale, 1)))
//                      saturate(var(--glass-saturate, 1.25));
//
// 覆盖两类写法：`backdrop-filter` 声明本身，以及存放滤镜函数的
// `--*-blur` 自定义属性（如 --menu-glass-blur: blur(28px) saturate(1.4)）。
//
// 回退值保持原样，所以未设置旋钮的强度档（rich）渲染结果不变；
// liquid / minimal / 无障碍分支则能一次性调节全部玻璃表面。
// 幂等：逐个函数判断，已接旋钮的跳过。
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../src/styles/", import.meta.url));
const DECLARATION = /((?:(?:-webkit-)?backdrop-filter|--[a-z0-9-]*blur)\s*:\s*)([^;}]+)/gi;
// 单层嵌套足够覆盖 blur(var(--x, 14px))
const INNER = "(?:[^()]|\\([^()]*\\))*";
const BLUR = new RegExp(`\\bblur\\(\\s*(${INNER}?)\\s*\\)`, "g");
const SATURATE = new RegExp(`\\bsaturate\\(\\s*(${INNER}?)\\s*\\)`, "g");
const PLAIN_LENGTH = /^-?[\d.]+(px|rem|em)$/;

function cssFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return cssFiles(path);
    return name.endsWith(".css") ? [path] : [];
  });
}

let touchedFiles = 0;
let blurCount = 0;
let saturateCount = 0;

for (const file of cssFiles(ROOT)) {
  const before = readFileSync(file, "utf8");
  const after = before.replace(DECLARATION, (match, property, value) => {
    const next = value
      .replace(BLUR, (blur, inner) => {
        if (!inner || inner.includes("--glass-blur-scale")) return blur;
        blurCount += 1;
        const operand = PLAIN_LENGTH.test(inner.trim()) ? inner.trim() : `(${inner.trim()})`;
        return `blur(calc(${operand} * var(--glass-blur-scale, 1)))`;
      })
      .replace(SATURATE, (saturate, inner) => {
        if (!inner || inner.includes("--glass-saturate")) return saturate;
        saturateCount += 1;
        return `saturate(var(--glass-saturate, ${inner.trim()}))`;
      });
    return `${property}${next}`;
  });
  if (after === before) continue;
  writeFileSync(file, after);
  touchedFiles += 1;
  console.log("rewrote", relative(ROOT, file));
}

console.log(
  `\n${touchedFiles} files · ${blurCount} blur() · ${saturateCount} saturate() rewired`,
);
