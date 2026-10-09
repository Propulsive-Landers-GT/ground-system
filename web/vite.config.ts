import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` proxies /ws to a bridge on this machine. Override with GS_BRIDGE=host:port.
const bridge = process.env.GS_BRIDGE ?? "127.0.0.1:8080";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/ws": { target: `ws://${bridge}`, ws: true, changeOrigin: true },
    },
  },
  build: { outDir: "dist", chunkSizeWarningLimit: 1500 },
});
