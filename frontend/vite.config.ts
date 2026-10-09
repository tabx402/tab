import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  server: { port: 5197, proxy: { "/api": loadEnv(mode, ".", "TAB_").TAB_API_ORIGIN || "http://127.0.0.1:4297" } },
  build: { chunkSizeWarningLimit: 1500 },
}));
