import { defineConfig, devices } from "@playwright/test";

const port = 6006;
const chromiumExecutable = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH;

export default defineConfig({
  testDir: "./visual-tests",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  // Storybook dev 单实例在满并发下会偶发「story 还没渲染出来」，
  // 表现为与改动无关的随机失败：限制并发并保留一次本地重试。
  workers: process.env.CI ? 2 : 3,
  retries: process.env.CI ? 2 : 1,
  reporter: process.env.CI ? "github" : "list",
  snapshotPathTemplate: "{testDir}/__screenshots__/{testFilePath}/{arg}{ext}",
  expect: {
    // iframe 首次命中要现编译 story 模块，5s 对冷启动偏紧。
    timeout: 10_000,
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
