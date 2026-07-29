import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// 版本号和 CSP 在构建期从各自的唯一出处注入。
//
// 「全程离线」那个弹层里会把真实的 CSP 原样贴出来 —— 声称和能验证是两回事。
// 但如果在界面里手抄一份，改了配置忘了改文案，那句「不信自己看」就成了假的。
// 从源头读，就不会有第二份。
const pkg = JSON.parse(readFileSync("./package.json", "utf8"));
const tauriConf = JSON.parse(readFileSync("./src-tauri/tauri.conf.json", "utf8"));

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_CSP__: JSON.stringify(tauriConf.app.security.csp),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
