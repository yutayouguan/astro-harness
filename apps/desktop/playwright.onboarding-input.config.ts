import { defineConfig, devices } from "@playwright/test";
import base from "./playwright.config";

/** Optional focused engine regression: npx playwright install chromium webkit */
export default defineConfig({
  ...base,
  testMatch: "onboarding-input.spec.ts",
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], browserName: "chromium" },
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"], browserName: "webkit", launchOptions: {} },
    },
  ],
});
