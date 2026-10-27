import react from "@vitejs/plugin-react";
import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import type { ClientRequest, IncomingMessage } from "node:http";
import { defineConfig, type Plugin, type ProxyOptions } from "vite";

// Opt-in native development proxy. The bearer stays in this Node process;
// it is never injected into Vite's client environment or returned to a tab.
const proxyRoot = process.env.MSV_UI_DATA_DIR;
const proxyTarget = process.env.MSV_UI_API_URL;
const hasNativeProxy = Boolean(proxyRoot && proxyTarget);
if (hasNativeProxy) {
  const target = new URL(proxyTarget!);
  if (target.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(target.hostname)) {
    throw new Error("MSV_UI_API_URL must be a loopback HTTP endpoint");
  }
}
const proxy: ProxyOptions | undefined = hasNativeProxy ? {
  target: proxyTarget, ws: true, changeOrigin: false,
  configure(server) {
    const authorize = (outgoing: ClientRequest, request: IncomingMessage) => {
      if ((request.headers.origin && request.headers.origin !== `http://${request.headers.host}`)
        || request.headers["sec-fetch-site"] === "cross-site") {
        outgoing.destroy(new Error("Origin is not allowed")); return;
      }
      try {
        const path = join(proxyRoot!, "run", "control.token");
        if ((statSync(path).mode & 0o077) !== 0) throw new Error("Control token must be owner-only");
        outgoing.setHeader("Authorization", `Bearer ${readFileSync(path, "utf8").trim()}`);
      } catch {
        outgoing.destroy(new Error("Native proxy authentication unavailable"));
      }
    };
    server.on("proxyReq", authorize);
    server.on("proxyReqWs", authorize);
  },
} : undefined;
const localRequestsOnly: Plugin = {
  name: "supervisor-local-proxy-origin",
  configureServer(server) {
    if (!hasNativeProxy) return;
    server.middlewares.use((request, response, next) => {
      if (!request.url?.startsWith("/api/")) return next();
      const origin = request.headers.origin;
      const expected = `http://${request.headers.host}`;
      if ((origin && origin !== expected) || request.headers["sec-fetch-site"] === "cross-site") {
        response.writeHead(403); response.end("Origin is not allowed"); return;
      }
      next();
    });
  },
};

export default defineConfig(({mode}) => ({
  resolve: {alias: mode === "desktop-e2e" ? [{
    find: /^@tauri-apps\/api\/core$/,
    replacement: new URL("./src/testing/tauri-core.ts", import.meta.url).pathname,
  }] : []},
  plugins: [react(), localRequestsOnly],
  define: {
    "import.meta.env.VITE_NATIVE_PROXY": JSON.stringify(hasNativeProxy),
    "import.meta.env.VITE_DESKTOP_E2E": JSON.stringify(mode === "desktop-e2e"),
    "import.meta.env.VITE_E2E_OS_OBSERVE": JSON.stringify(mode === "desktop-e2e" && process.env.MSV_E2E_OS_OBSERVE === "1"),
    "import.meta.env.VITE_E2E_REVISION": JSON.stringify(mode === "desktop-e2e" ? process.env.MSV_E2E_REVISION : null),
    "import.meta.env.VITE_E2E_SOURCE_SHA256": JSON.stringify(mode === "desktop-e2e" ? process.env.MSV_E2E_SOURCE_SHA256 : null),
  },
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    proxy: proxy ? { "/api": proxy } : undefined,
  },
}));
