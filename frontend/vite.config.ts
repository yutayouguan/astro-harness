import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // 避免依赖树解析出多份 React，导致 Context Provider / useContext 失配
  resolve: {
    dedupe: ["react", "react-dom"],
  },
  optimizeDeps: {
    include: ["react", "react-dom"],
  },
  server: {
    port: 1420,
    strictPort: true,
    // Tauri WebView 常把 localhost 解析到 127.0.0.1；避免只绑 [::1] 导致空白窗
    host: host || "127.0.0.1",
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : { host: "127.0.0.1" },
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
