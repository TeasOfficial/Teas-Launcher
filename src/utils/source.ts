/**
 * 下载源参数映射 — 前端唯一实现
 *
 * 后端的游戏下载源判断写的是 `source == "Mojang"`（见 instance.rs / install/vanilla.rs），
 * 而配置里存的是 `game_source: "官方源" | "BMCLAPI"`。直接把配置值传过去永远不等于
 * "Mojang"，于是用户选了官方源也照样走镜像。所有需要传游戏源的地方都必须经过这里。
 */
import { invoke } from "@tauri-apps/api/core";

/** 把配置里的 game_source 转成后端期望的参数值 */
export function gameSourceParam(cfg: Record<string, any> | null | undefined): string {
  return cfg?.game_source === "BMCLAPI" ? "BMCLAPI" : "Mojang";
}

/** 读取用户配置（失败返回空对象，调用方按默认值处理） */
export async function readUserConfig(): Promise<Record<string, any>> {
  try {
    return await invoke<Record<string, any>>("config_read", { scope: "user" });
  } catch {
    return {};
  }
}
