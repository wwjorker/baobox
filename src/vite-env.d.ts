/// <reference types="vite/client" />

/** 构建期注入，见 vite.config.ts。唯一出处分别是 package.json 和 tauri.conf.json。 */
declare const __APP_VERSION__: string;
declare const __APP_CSP__: string;
