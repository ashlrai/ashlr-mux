import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The desktop web shell. Vite drives both `tauri dev` (HMR dev server on a
// fixed port so `tauri.conf.json` `build.devUrl` can point at it) and the
// release build (static `dist/` consumed via `frontendDist`).
//
// A fixed, strict port is required: Tauri launches this dev server via
// `beforeDevCommand` and then loads `devUrl`; if Vite silently picked another
// port the WebView2 window would load a blank page.
const DEV_PORT = 1420;

export default defineConfig({
  // Root is this package; entry is `index.html` at the package root.
  clearScreen: false,
  plugins: [react(), tailwindcss()],
  server: {
    port: DEV_PORT,
    strictPort: true,
    // Tauri hosts the app from a custom protocol origin (not localhost), so the
    // HMR websocket must be reachable at an explicit host/port.
    host: "127.0.0.1",
    hmr: { host: "127.0.0.1", port: DEV_PORT },
    watch: {
      // The Rust backend is rebuilt by cargo/tauri, not Vite — don't churn HMR
      // on target/ or the Tauri crate.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // WebView2 is evergreen Chromium; target modern output.
    target: "esnext",
    sourcemap: false,
  },
});
