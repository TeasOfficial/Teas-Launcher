/**
 * 配置读写 — 前端唯一入口（user = %APPDATA%/TeasLauncher/tc.ini，launcher = .teas/tc.ini）
 *
 * 以前每个组件各写一遍 `invoke("config_read", { scope: "user" })` + try/catch，
 * scope 字面量散落各处、失败也基本被吞掉。统一收在这里，读失败返回 {}，
 * 写失败返回 false 并留一条 warn，调用方不需要重复样板。
 */
import { invoke } from "@tauri-apps/api/core";

export type ConfigScope = "user" | "launcher";

/** 读取配置（失败返回空对象，调用方按默认值处理） */
export async function readConfig(scope: ConfigScope): Promise<Record<string, any>> {
  try {
    return await invoke<Record<string, any>>("config_read", { scope });
  } catch (e) {
    console.warn("[config] 读取失败:", scope, e);
    return {};
  }
}

/** 写入单个 key（后端合并写入，不影响其他字段）。失败返回 false。 */
export async function writeConfig(
  scope: ConfigScope,
  key: string,
  value: unknown
): Promise<boolean> {
  try {
    await invoke("config_write", { scope, key, value });
    return true;
  } catch (e) {
    console.warn("[config] 保存失败:", scope, key, e);
    return false;
  }
}

export const readUserConfig = () => readConfig("user");
export const writeUserConfig = (key: string, value: unknown) => writeConfig("user", key, value);
export const readLauncherConfig = () => readConfig("launcher");
export const writeLauncherConfig = (key: string, value: unknown) =>
  writeConfig("launcher", key, value);
