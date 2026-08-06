/**
 * 前后端契约静态检查
 *
 * 后端有几百条断言，但它们抓不到「前端与后端接不上」这一类 bug——
 * registry 里的参数名和 Rust 命令参数对不上、命令没注册、某个 i18n key
 * 只有中文没英文……这些编译都能过，测试也照样绿，可工具在界面上会静默失效。
 * （曾经缩略图功能就是这么整个哑掉的：后端全对、编译通过，前端一行接线错了。）
 *
 * 这个脚本把这类契约用纯静态的方式核一遍，发现问题就非零退出。
 *
 *   node scripts/check-contract.mjs      （或 npm run check）
 *
 * 三块检查：
 *   A. registry ↔ 后端命令：每个 ready 工具的 command 有对应且已注册的
 *      #[tauri::command]；ToolRunner 自动传参的 option id 能对上 Rust 参数名。
 *   B. 前端所有 invoke("cmd", {..}) 手写调用的参数名对得上 Rust 参数。
 *   C. i18n：中英文 key 集合完全一致；每个工具的 name/desc、每个 option 的
 *      label/placeholder、后端发出的 err.* key 在两种语言里都有。
 *
 * Tauri v2 会把 Rust 的 snake_case 参数名自动转成前端的 camelCase，所以比对
 * 前统一折成 camelCase。app/window/state 这类由 Tauri 注入、不从前端传。
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const rustDir = path.join(ROOT, "src-tauri/src");
const problems = [];
const p = (s) => problems.push(s);

const toCamel = (s) => s.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase());
const INJECTED = new Set(["app", "window", "webview", "state", "app_handle"]);

// ---- 解析 Rust #[tauri::command] 函数及其参数 -------------------------------
const cmds = {};
for (const f of fs.readdirSync(rustDir).filter((x) => x.endsWith(".rs"))) {
  const src = fs.readFileSync(path.join(rustDir, f), "utf8");
  const re =
    /#\[tauri::command\][^\n]*\n(?:\s*#\[[^\]]*\]\s*\n)*\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*\(/g;
  let m;
  while ((m = re.exec(src))) {
    let d = 0, j = re.lastIndex - 1;
    for (; j < src.length; j++) {
      if (src[j] === "(") d++;
      else if (src[j] === ")") { d--; if (d === 0) break; }
    }
    const paramStr = src.slice(re.lastIndex, j);
    const parts = [];
    let dd = 0, last = 0;
    for (let k = 0; k < paramStr.length; k++) {
      const ch = paramStr[k];
      if ("<([".includes(ch)) dd++;
      else if (">)]".includes(ch)) dd--;
      else if (ch === "," && dd === 0) { parts.push(paramStr.slice(last, k)); last = k + 1; }
    }
    parts.push(paramStr.slice(last));
    cmds[m[1]] = parts
      .map((s) => s.trim()).filter(Boolean)
      .map((s) => (s.match(/^(\w+)\s*:/) || [])[1]).filter(Boolean);
  }
}

// ---- generate_handler! 注册表 ----------------------------------------------
const lib = fs.readFileSync(path.join(rustDir, "lib.rs"), "utf8");
const gen = lib.indexOf("generate_handler!");
const regBlock = lib.slice(gen, lib.indexOf("]", gen));
const registered = new Set(
  [...regBlock.matchAll(/([a-z_][a-z0-9_]*)\s*(?:,|\n|\])/gi)].map((m) => m[1]),
);

// ---- 解析 registry 工具 -----------------------------------------------------
const reg = fs.readFileSync(path.join(ROOT, "src/tools/registry.ts"), "utf8");
const s0 = reg.indexOf("export const TOOLS");
const arrOpen = reg.indexOf("[", reg.indexOf("=", s0));
{
  let depth = 0, cur = null;
  const objs = [];
  for (let i = arrOpen; i < reg.length; i++) {
    const c = reg[i];
    if (c === "[") depth++;
    else if (c === "]") { depth--; if (depth === 0) break; }
    else if (c === "{") { if (depth === 1 && cur === null) cur = i; depth++; }
    else if (c === "}") { depth--; if (depth === 1 && cur !== null) { objs.push(reg.slice(cur, i + 1)); cur = null; } }
  }
  var tools = objs.map((body) => {
    const ids = [...body.matchAll(/\bid:\s*"([^"]+)"/g)].map((m) => m[1]);
    return {
      id: ids[0],
      command: (body.match(/\bcommand:\s*"([^"]+)"/) || [])[1],
      status: (body.match(/\bstatus:\s*"([^"]+)"/) || [])[1],
      optionIds: ids.slice(1),
    };
  });
}

// 走独立面板、自己手搓 invoke 的工具：option id 不由 ToolRunner 自动传，跳过 A 的参数比对
const HAND_BUILT = new Set([
  "image.redact", "file.dedupe", "file.rename", "ocr.screen",
  "file.shred", "file.touch", "pdf.split",
]);

// ---- A. registry ↔ 后端 -----------------------------------------------------
for (const t of tools) {
  if (t.status === "planned") continue;
  if (!cmds[t.command]) { p(`A [缺函数] 工具 ${t.id}: 命令 "${t.command}" 没有对应的 #[tauri::command]`); continue; }
  if (!registered.has(t.command)) p(`A [未注册] 工具 ${t.id}: 命令 "${t.command}" 不在 generate_handler!`);
  if (HAND_BUILT.has(t.id)) continue;
  const rustCamel = new Set(cmds[t.command].filter((x) => !INJECTED.has(x)).map(toCamel));
  for (const oid of t.optionIds)
    if (!rustCamel.has(oid))
      p(`A [参数对不上] ${t.id} (${t.command}): option "${oid}" 无对应 Rust 参数 [${cmds[t.command].join(", ")}]`);
}

// ---- B. 前端所有 invoke("cmd", {..}) ----------------------------------------
function walk(dir) {
  let out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const fp = path.join(dir, e.name);
    if (e.isDirectory()) out = out.concat(walk(fp));
    else if (/\.(ts|tsx)$/.test(e.name)) out.push(fp);
  }
  return out;
}
for (const fp of walk(path.join(ROOT, "src"))) {
  const src = fs.readFileSync(fp, "utf8");
  const re = /invoke\s*(?:<[^>]*>)?\s*\(\s*"([^"]+)"\s*(?:,\s*([\s\S]*?))?\)/g;
  let m;
  while ((m = re.exec(src))) {
    const name = m[1];
    const arg = (m[2] || "").trim();
    if (!cmds[name]) { p(`B [无此命令] invoke("${name}") @ ${path.relative(ROOT, fp)}`); continue; }
    if (!arg.startsWith("{")) continue; // 参数是变量，无法静态取 key
    let d = 0, end = -1;
    for (let k = 0; k < arg.length; k++) {
      if (arg[k] === "{") d++;
      else if (arg[k] === "}") { d--; if (d === 0) { end = k; break; } }
    }
    const objBody = arg.slice(1, end);
    const keys = [];
    let dd = 0, tok = 0;
    const push = (seg) => {
      seg = seg.trim();
      const k = (seg.match(/^([A-Za-z0-9_]+)\s*:/) || seg.match(/^([A-Za-z0-9_]+)\s*$/) || [])[1];
      if (k) keys.push(k);
    };
    for (let k = 0; k < objBody.length; k++) {
      const ch = objBody[k];
      if ("{[(".includes(ch)) dd++;
      else if ("}])".includes(ch)) dd--;
      else if (ch === "," && dd === 0) { push(objBody.slice(tok, k)); tok = k + 1; }
    }
    push(objBody.slice(tok));
    const rustCamel = new Set(cmds[name].filter((x) => !INJECTED.has(x)).map(toCamel));
    for (const key of keys)
      if (!rustCamel.has(key))
        p(`B [参数对不上] invoke("${name}", {${keys.join(", ")}}) @ ${path.relative(ROOT, fp)}: "${key}" 无对应 Rust 参数 [${cmds[name].join(", ")}]`);
  }
}

// ---- C. i18n 完整性 ---------------------------------------------------------
function loadLocale(file, anchor) {
  const src = fs.readFileSync(path.join(ROOT, file), "utf8");
  const open = src.indexOf("{", src.indexOf(anchor));
  let d = 0, end = -1;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") d++;
    else if (src[i] === "}") { d--; if (d === 0) { end = i; break; } }
  }
  return new Function("return (" + src.slice(open, end + 1) + ")")();
}
const flatten = (o, pre, out) => {
  for (const [k, v] of Object.entries(o)) {
    const key = pre ? pre + "." + k : k;
    if (v && typeof v === "object" && !Array.isArray(v)) flatten(v, key, out);
    else out.add(key);
  }
  return out;
};
const zhKeys = flatten(loadLocale("src/i18n/zh-CN.ts", "export default"), "", new Set());
const enKeys = flatten(loadLocale("src/i18n/en-US.ts", "const enUS"), "", new Set());
for (const k of zhKeys) if (!enKeys.has(k)) p(`C [en 缺] key "${k}" 有中文无英文`);
for (const k of enKeys) if (!zhKeys.has(k)) p(`C [zh 缺] key "${k}" 有英文无中文`);

const isKey = (s) => /^[a-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)+$/.test(s);
const need = new Set();
for (const t of tools) {
  if (t.id) { need.add(`tool.${t.id}.name`); need.add(`tool.${t.id}.desc`); }
}
for (const m of reg.matchAll(/\b(?:label|placeholder|notReadyReason):\s*"([^"]+)"/g)) {
  if (isKey(m[1])) need.add(m[1]);
}
for (const k of need) {
  if (!zhKeys.has(k)) p(`C [zh 缺] 工具/选项 key "${k}"`);
  if (!enKeys.has(k)) p(`C [en 缺] 工具/选项 key "${k}"`);
}
const emitted = new Set();
for (const f of fs.readdirSync(rustDir).filter((x) => x.endsWith(".rs"))) {
  const src = fs.readFileSync(path.join(rustDir, f), "utf8");
  for (const m of src.matchAll(/"(err\.[a-zA-Z0-9_.]+)"/g)) emitted.add(m[1]);
}
for (const k of emitted) {
  if (!zhKeys.has(k)) p(`C [zh 缺] 后端发出 "${k}" 无中文`);
  if (!enKeys.has(k)) p(`C [en 缺] 后端发出 "${k}" 无英文`);
}

// ---- 汇报 -------------------------------------------------------------------
const ready = tools.filter((t) => t.status === "ready").length;
const planned = tools.filter((t) => t.status === "planned").length;
console.log(`工具 ${tools.length}（ready ${ready} / planned ${planned}）· Rust 命令 ${Object.keys(cmds).length} · i18n key 中 ${zhKeys.size} / 英 ${enKeys.size}`);
if (problems.length === 0) {
  console.log("契约检查通过：前后端参数、命令注册、中英文案全部对得上。");
  process.exit(0);
} else {
  console.error(`\n发现 ${problems.length} 个契约问题：`);
  for (const x of problems) console.error("  " + x);
  process.exit(1);
}
