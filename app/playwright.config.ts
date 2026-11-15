import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests",
  timeout: 30000,
  use: {
    baseURL: "http://localhost:1465",
  },
  webServer: {
    command: "npm run dev",
    port: 1465,
    reuseExistingServer: false,
    timeout: 60000,
  },
});
