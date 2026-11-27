import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// The wallet website talks to two things on this computer: the bridge (the device and the chain
// proxy, http://127.0.0.1:7630) and nothing else. No analytics, no CDN, no external fonts.
// From the create-qorechain-dapp starter: /lcd/* is forwarded to QORE_REST_PROXY_TARGET (.env), because the public
// REST API sends no CORS headers. The wallet itself goes through the bridge's /proxy instead (src/networks.ts);
// the starter's own screen (src/starter/App.tsx) uses /lcd.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ".", "");
  const target = env.QORE_REST_PROXY_TARGET || "http://localhost:1317";
  const proxy = {
    "/lcd": { target, changeOrigin: true, rewrite: (p: string) => p.replace(/^\/lcd/, "") },
  };
  return {
    plugins: [react()],
    server: { host: "127.0.0.1", port: 5173, strictPort: true, proxy },
    preview: { proxy },
    build: { target: "es2022", sourcemap: false },
  };
});
