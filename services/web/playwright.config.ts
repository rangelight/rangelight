// End-to-end tests of the Workbench against the stub api (test/e2e/stub.ts). The screenshots in
// test/screenshots/ are the review artifact: each view in light and dark.
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "test/e2e",
  snapshotPathTemplate: "test/screenshots/{arg}{ext}",
  // One stub, shared state: tests run one at a time.
  workers: 1,
  fullyParallel: false,
  forbidOnly: true,
  timeout: 60_000,
  globalSetup: "./test/e2e/global-setup.ts",
  use: {
    baseURL: "http://127.0.0.1:4173",
    viewport: { width: 1100, height: 660 },
    // Stable times and formats in the screenshots.
    locale: "en-GB",
    timezoneId: "UTC",
    browserName: "chromium",
  },
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.02, animations: "disabled", caret: "hide" } },
  // The production build, as the image serves it.
  webServer: {
    command: "vite build && vite preview",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: false,
  },
});
