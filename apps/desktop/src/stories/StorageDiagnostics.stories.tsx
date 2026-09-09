import type { Meta, StoryObj } from "@storybook/react-vite";
import StorageDiagnostics from "../components/settings/StorageDiagnostics";
import { ThemeProvider } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { StorageReport } from "../lib/settings/storageDiagnostics";

const report: StorageReport = {
  rootPath: "/Users/demo/.astro",
  configPath: "/Users/demo/.astro/config.toml",
  state: "ready",
  settingsVersion: 1,
  configPresent: true,
  partial: false,
  inspectedEntries: 120,
  domains: [
    {
      id: "models",
      bytes: 3250585,
      files: 19,
      skippedLinks: 0,
      previewBytes: 131072,
      previewFiles: 1,
    },
    {
      id: "sessions",
      bytes: 4194304,
      files: 14,
      skippedLinks: 0,
      previewBytes: 0,
      previewFiles: 0,
    },
    {
      id: "browser",
      bytes: 209715200,
      files: 66,
      skippedLinks: 1,
      previewBytes: 0,
      previewFiles: 0,
    },
    {
      id: "workspace",
      bytes: 5242880,
      files: 21,
      skippedLinks: 0,
      previewBytes: 0,
      previewFiles: 0,
    },
  ],
  issues: [
    { code: "resource_missing", path: "ui/wallpapers/missing-reference.png" },
  ],
  cleanupPreview: [
    {
      path: "models/cache/old-model-metadata.json",
      bytes: 131072,
      policy: "cache_expired",
    },
  ],
  previewPartial: true,
  cachePolicies: [
    {
      domain: "models",
      directory: "/Users/demo/.astro/models/cache",
      enabled: true,
      ttlSeconds: 600,
      maxSizeMb: 256,
      status: "in_home",
    },
    {
      domain: "mcp",
      directory: "/Volumes/cache/astro-mcp",
      enabled: true,
      ttlSeconds: 1800,
      maxSizeMb: 128,
      status: "external",
    },
  ],
};
const meta = {
  title: "Settings/StorageDiagnostics",
  component: StorageDiagnostics,
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <main
            style={{
              padding: 24,
              maxWidth: 920,
              margin: "0 auto",
              color: "var(--ink)",
              background: "var(--shell-bg)",
              minHeight: "100vh",
              boxSizing: "border-box",
            }}
          >
            <Story />
          </main>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  beforeEach: () => localStorage.setItem("astro-locale", "zh"),
  args: { active: true, initialReport: report },
} satisfies Meta<typeof StorageDiagnostics>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Ready: Story = {};
export const InvalidConfig: Story = {
  args: {
    initialReport: {
      ...report,
      state: "invalid_config",
      settingsVersion: null,
      issues: [{ code: "config_invalid", path: "config.toml" }],
    },
  },
};
export const Partial: Story = {
  args: { initialReport: { ...report, partial: true } },
};
export const NewInstall: Story = {
  args: {
    initialReport: {
      ...report,
      state: "new_install",
      configPresent: false,
      settingsVersion: null,
      domains: [],
      issues: [],
      cleanupPreview: [],
    },
  },
};
