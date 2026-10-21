#!/usr/bin/env node
/**
 * 把 pre-1.6（Beta / Alpha 时代）的 legacy 资源下载到 resources/ 目录。
 *
 * 背景：这类版本在启动时会去拉 http://s3.amazonaws.com/MinecraftResources/，
 * 那个桶早已下线（日志里是 FileNotFoundException），结果就是没有声音 / 没有 lang。
 * 官方启动器的做法是把 assets 索引里 map_to_resources=true 的对象**摊平**复制到
 * <gameDir>/resources/，游戏就会直接读本地文件。
 *
 * 用法:
 *   node scripts/fetch-legacy-assets.mjs "<versions/b1.6.4/b1.6.4.json>"
 *   node scripts/fetch-legacy-assets.mjs "<json>" --out "D:\...\versions\b1.6.4\resources"
 *   node scripts/fetch-legacy-assets.mjs "<json>" --jobs 8 --mirror
 */

import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";

const args = process.argv.slice(2);
if (!args.length) {
  console.error("用法: node scripts/fetch-legacy-assets.mjs <version.json> [--out <dir>] [--jobs N] [--mirror]");
  process.exit(1);
}
const versionJsonPath = path.resolve(args[0]);
const outIdx = args.indexOf("--out");
const jobsIdx = args.indexOf("--jobs");
const preferMirror = args.includes("--mirror");
const outDir = outIdx >= 0 ? path.resolve(args[outIdx + 1]) : path.join(path.dirname(versionJsonPath), "resources");
const jobs = jobsIdx >= 0 ? Math.max(1, Math.min(16, parseInt(args[jobsIdx + 1], 10) || 8)) : 8;

const OFFICIAL = "https://resources.download.minecraft.net";
const MIRROR = "https://bmclapi2.bangbang93.com/assets";
const bases = preferMirror ? [MIRROR, OFFICIAL] : [OFFICIAL, MIRROR];

const version = JSON.parse(await fsp.readFile(versionJsonPath, "utf8"));
const idx = version.assetIndex;
if (!idx || !idx.url) {
  console.error("该版本 JSON 没有 assetIndex.url，无法处理");
  process.exit(1);
}
if (idx.id === "legacy" || (version.assets && !String(version.assets).startsWith("pre-1.6"))) {
  console.log(`提示: assets=${version.assets}，这里只对 pre-1.6 这类 map_to_resources 索引有效`);
}

async function fetchText(url, tries = 3) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(url);
      if (!r.ok) throw new Error("HTTP " + r.status);
      return await r.text();
    } catch (e) {
      if (i === tries - 1) throw e;
      await new Promise((s) => setTimeout(s, 400 * (i + 1)));
    }
  }
}

async function fetchBin(url, tries = 3) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(url);
      if (!r.ok) throw new Error("HTTP " + r.status);
      return Buffer.from(await r.arrayBuffer());
    } catch (e) {
      if (i === tries - 1) throw e;
      await new Promise((s) => setTimeout(s, 300 * (i + 1)));
    }
  }
}

console.log("索引: " + idx.url);
const index = JSON.parse(await fetchText(idx.url));
if (!index.map_to_resources) {
  console.error("该索引 map_to_resources=false，不需要摊平到 resources/");
  process.exit(1);
}
const entries = Object.entries(index.objects);
const totalBytes = entries.reduce((n, [, v]) => n + v.size, 0);
console.log(`对象 ${entries.length} 个，合计 ${(totalBytes / 1048576).toFixed(1)} MB`);
console.log("目标目录: " + outDir);

let done = 0, skipped = 0, failed = 0;
let cursor = 0;
const failures = [];

async function worker() {
  while (cursor < entries.length) {
    const [key, obj] = entries[cursor++];
    const dest = path.join(outDir, key.split("/").join(path.sep));
    try {
      const st = await fsp.stat(dest).catch(() => null);
      if (st && st.size === obj.size) { skipped++; continue; }
      await fsp.mkdir(path.dirname(dest), { recursive: true });
      const p = obj.hash.slice(0, 2) + "/" + obj.hash;
      let buf, lastErr;
      for (const base of bases) {
        try { buf = await fetchBin(base + "/" + p); break; } catch (e) { lastErr = e; }
      }
      if (!buf) throw lastErr || new Error("全部源失败");
      await fsp.writeFile(dest, buf);
      done++;
    } catch (e) {
      failed++;
      failures.push(key + ": " + e.message);
    }
    const n = done + skipped + failed;
    if (n % 100 === 0 || n === entries.length) {
      process.stdout.write(`\r进度 ${n}/${entries.length}  新增 ${done}  跳过 ${skipped}  失败 ${failed}`);
    }
  }
}

const t0 = Date.now();
await Promise.all(Array.from({ length: jobs }, worker));
console.log(`\n完成: 新增 ${done} / 跳过 ${skipped} / 失败 ${failed}，用时 ${((Date.now() - t0) / 1000).toFixed(1)}s`);
if (failures.length) {
  console.log("失败清单（前 10）:");
  failures.slice(0, 10).forEach((f) => console.log("  " + f));
  process.exitCode = 2;
}
