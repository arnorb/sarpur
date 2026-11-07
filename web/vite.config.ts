import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// `npm run dev` serves the UI on :5173 and passes /api to a running sarpur
// (`cargo run -p sarpur`, :8090). `npm run build` writes dist/, which the
// sarpur binary embeds.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(__dirname, "./src") } },
  server: {
    proxy: { "/api": { target: process.env.SARPUR_URL ?? "http://127.0.0.1:8090", changeOrigin: false } },
  },
});
