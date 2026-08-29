import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const shim = (name: string) => path.resolve(__dirname, "src/lib", name);

// Web 管理面板构建：复用 ../src 全部页面与组件，仅把 Tauri API 替换为 HTTP 实现。
export default defineConfig({
  plugins: [react(), tailwindcss()],
  publicDir: path.resolve(__dirname, "../public"),
  resolve: {
    alias: [
      // 主前端源码树（pages/components/hooks/types/lib）
      { find: "@app", replacement: path.resolve(__dirname, "../src") },
      // Tauri API → Web shim
      { find: "@tauri-apps/api/core", replacement: shim("tauri-shim.ts") },
      { find: "@tauri-apps/api/event", replacement: shim("tauri-event-shim.ts") },
      { find: "@tauri-apps/api/app", replacement: shim("tauri-app-shim.ts") },
      { find: "@tauri-apps/plugin-opener", replacement: shim("plugin-opener-shim.ts") },
      { find: "@tauri-apps/plugin-dialog", replacement: shim("plugin-dialog-shim.ts") },
      { find: "@tauri-apps/plugin-updater", replacement: shim("plugin-updater-shim.ts") },
      { find: "@tauri-apps/plugin-process", replacement: shim("plugin-process-shim.ts") },
    ],
  },
  server: {
    port: 1420,
    strictPort: true,
    proxy: {
      "/admin/api": "http://127.0.0.1:8777",
      "/api": "http://127.0.0.1:8777",
      "/v1": "http://127.0.0.1:8777",
      "/mcp": "http://127.0.0.1:8777",
      "/health": "http://127.0.0.1:8777",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 2048,
  },
});
