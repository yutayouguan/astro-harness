import { defineConfig, devices } from "@playwright/test";

const port = 6006;
const chromiumExecutable = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH;

export default defineConfig({
  testDir: "./visual-tests",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "list",
  snapshotPathTemplate: "{testDir}/__screenshots__/{testFilePath}/{arg}{ext}",
  expect: {
    toHaveScreenshot: {
      animations: "disabled",
      maxDiffPixelRatio: 0.02,
    },
  },
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    ...devices["Desktop Chrome"],
    browserName: "chromium",
    deviceScaleFactor: 1,
    launchOptions: chromiumExecutable
      ? { executablePath: chromiumExecutable }
      : undefined,
    trace: "on-first-retry",
  },
  webServer: {
    command: process.env.CI
      ? `http-server storybook-static -a 127.0.0.1 -p ${port} -c-1`
      : `npm run storybook -- --ci --host 127.0.0.1`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
