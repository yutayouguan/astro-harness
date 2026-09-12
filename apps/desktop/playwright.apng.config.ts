import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./visual-tests",
  testMatch: "pet-apng.spec.ts",
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:6323",
    viewport: { width: 1000, height: 750 },
  },
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  webServer: {
    command: "npm run storybook -- --ci --host 127.0.0.1 --port 6323",
    url: "http://127.0.0.1:6323",
    reuseExistingServer: true,
    timeout: 120000,
  },
});
