import { defineConfig } from "@playwright/test";

// Test real Vite output, not Storybook's different CSS import graph.
export default defineConfig({
  testDir: "./production-tests",
  testMatch: "font-cascade.spec.ts",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:6322",
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
        launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
          ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH }
          : undefined,
      },
    },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  webServer: {
    command: "vite preview --host 127.0.0.1 --port 6322 --strictPort",
    url: "http://127.0.0.1:6322",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
