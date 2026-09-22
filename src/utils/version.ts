/**
 * Minecraft 版本号比较 — 前端唯一实现
 *
 * 后端 launch/java.rs 的 compare_versions 负责启动期的版本判定；
 * 本模块只服务于 UI 侧的兼容性提示与筛选。
 */

/** 提取 "1.20.1" 中的数字部分；无法识别时返回 [0,0,0] */
export function parseMcVersion(ver: string): number[] {
  const m = ver.match(/^(\d+)\.(\d+)(?:\.(\d+))?/);
  if (!m) return [0, 0, 0];
  return [parseInt(m[1]), parseInt(m[2]), parseInt(m[3] || "0")];
}

/** 版本号比较：a > b 返回正数，a < b 返回负数，相等返回 0 */
export function versionCmp(a: string, b: string): number {
  const va = parseMcVersion(a);
  const vb = parseMcVersion(b);
  for (let i = 0; i < 3; i++) {
    if (va[i] !== vb[i]) return va[i] - vb[i];
  }
  return 0;
}
