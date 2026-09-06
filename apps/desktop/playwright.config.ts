import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests/ui",
  outputDir: "../../target/frontend-acceptance",
  workers: 1,
  reporter: "list",
  use: { baseURL: "http://127.0.0.1:1425", channel: "msedge", headless: true, viewport: { width: 1280, height: 800 }, screenshot: "only-on-failure", trace: "retain-on-failure" },
  webServer: { command: "npm run dev -- --host 127.0.0.1 --port 1425 --strictPort", url: "http://127.0.0.1:1425", reuseExistingServer: true },
});
