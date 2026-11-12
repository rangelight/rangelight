// The Workbench build. In development the api is the Playwright stub (test/e2e/stub.ts) on :4180;
// in production one origin serves both (ADR 0040 §4), so the app never names a host.
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const api = "http://127.0.0.1:4180";
const proxy = {
  "/auth": api,
  "/rangelight.auth.v1alpha1.AuthService": api,
  "/rangelight.session.v1alpha1.SessionService": api,
  "/v1alpha1": { target: api, ws: true },
};

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("src", import.meta.url)),
      // Generated from api/ (scripts/check.sh gen, buf generate); never edited by hand.
      "@gen": fileURLToPath(new URL("../../shared/ts/gen", import.meta.url)),
    },
  },
  server: { host: "127.0.0.1", port: 4173, strictPort: true, proxy },
  preview: { host: "127.0.0.1", port: 4173, strictPort: true, proxy },
  test: { include: ["src/**/*.test.ts"] },
});
