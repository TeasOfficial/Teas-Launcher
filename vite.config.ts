import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    // 本仓库是 Cargo workspace，target/ 在仓库根目录（不在 src-tauri 下）。
    // 只忽略 src-tauri 的话，Vite 会去 watch target/**/*.exe，cargo 一重写就 EBUSY 崩掉。
    watch: {
      ignored: [
        "**/src-tauri/**",
        "**/target/**",
        "**/dist/**",
        "**/release/**",
        "**/.teas/**",
      ],
    },
  },
}));
