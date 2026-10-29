import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;
// Tauri sets TAURI_ENV_DEBUG for `tauri dev` and `tauri build --debug`.
const tauriDebug = Boolean(process.env.TAURI_ENV_DEBUG) && process.env.TAURI_ENV_DEBUG !== "false";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
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
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "safari15",
    // Release builds ship minified without source maps.
    minify: tauriDebug ? false : "esbuild",
    sourcemap: tauriDebug,
  },
});
