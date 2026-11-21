import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The wallet website talks to two things on this computer: the bridge (the device and the chain
// proxy, http://127.0.0.1:7630) and nothing else. No analytics, no CDN, no external fonts.
export default defineConfig({
  plugins: [react()],
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  build: { target: "es2022", sourcemap: false },
});
