import { defineConfig, devices } from "@playwright/test";
import base from "./playwright.config";

/** Keep the cinematic transition and form input regression covered in both engines. */
export default defineConfig({
  ...base,
  testMatch: ["onboarding-warp.spec.ts", "onboarding-input.spec.ts", "onboarding-logo.spec.ts", "onboarding-shadow.spec.ts"],
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"], browserName: "chromium" } },
    { name: "webkit", use: { ...devices["Desktop Safari"], browserName: "webkit", launchOptions: {} } },
  ],
});
