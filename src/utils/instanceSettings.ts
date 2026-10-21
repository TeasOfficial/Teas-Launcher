/**
 * 实例级设置的合并逻辑 — 前端唯一实现
 *
 * 后端只保存**被显式覆盖**的字段（`versions/<实例名>/teas-instance.json`），
 * 缺字段表示"跟随全局"（用户级 tc.ini）。这里集中做合并与默认值兜底，
 * 避免调用方各写一份 `inst.x ?? global.x` 而漏掉某个回退——下载源当初就是
 * 在 Dashboard 和安装向导里各写一份，结果两边语义不一致。
 */
import { invoke } from "@tauri-apps/api/core";
import { readUserConfig } from "./config";

export interface LaunchSettings {
  javaPath: string;
  maxMemory: string;
  jvmArgs: string;
  windowWidth: number;
  windowHeight: number;
  /** 内存是否由实例覆写（UI 上用它标记"这一项不是全局值"） */
  memoryOverridden: boolean;
}

function isSet(v: unknown): boolean {
  return v !== undefined && v !== null && String(v).trim() !== "";
}

/** 读取实例覆盖项（无文件/损坏时返回空对象 = 全部跟随全局） */
export async function readInstanceConfig(
  mcDir: string,
  instanceName: string
): Promise<Record<string, any>> {
  try {
    return await invoke<Record<string, any>>("instance_config_read", {
      mcDir,
      instanceName,
    });
  } catch {
    return {};
  }
}

/** 合并实例覆盖项与全局配置：实例优先，其次全局，最后内置默认值 */
export function effectiveSettings(
  instCfg: Record<string, any> | null | undefined,
  globalCfg: Record<string, any> | null | undefined
): LaunchSettings {
  const pick = (key: string, fallback: string): string => {
    if (isSet(instCfg?.[key])) return String(instCfg![key]).trim();
    if (isSet(globalCfg?.[key])) return String(globalCfg![key]).trim();
    return fallback;
  };
  const num = (key: string, fallback: number): number => {
    const n = parseInt(pick(key, String(fallback)), 10);
    return Number.isFinite(n) && n > 0 ? n : fallback;
  };

  return {
    javaPath: pick("java_path", "javaw"),
    maxMemory: pick("max_memory", "4G"),
    jvmArgs: pick("jvm_args", "-XX:+UseG1GC"),
    windowWidth: num("window_width", 854),
    windowHeight: num("window_height", 480),
    memoryOverridden: isSet(instCfg?.max_memory),
  };
}

/** 一步读齐：全局配置 + 实例覆盖项 → 合成启动参数 */
export async function readLaunchSettings(
  mcDir: string,
  instanceName: string
): Promise<LaunchSettings> {
  const [globalCfg, instCfg] = await Promise.all([
    readUserConfig(),
    readInstanceConfig(mcDir, instanceName),
  ]);
  return effectiveSettings(instCfg, globalCfg);
}
