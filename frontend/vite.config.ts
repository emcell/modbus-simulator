import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      // Route the WebSocket subscription endpoint first; vite matches in
      // order and without `ws: true` the upgrade never happens → the
      // Traffic tab stays silent even though the backend is emitting.
      "/graphql/ws": {
        target: "ws://127.0.0.1:8080",
        ws: true,
        changeOrigin: true,
      },
      "/graphql": "http://127.0.0.1:8080",
      "/playground": "http://127.0.0.1:8080",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
