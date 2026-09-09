import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import StorageCleanup from "../components/settings/StorageCleanup";
import { LocaleProvider } from "../i18n/LocaleContext";
import { ThemeProvider } from "../hooks/app/useTheme";

const token = "00000000-0000-4000-8000-000000000001";
const path = `models/cache/astro-cache-v1-models-${"a".repeat(64)}.json`;
const meta = {
  title: "Settings/StorageCleanup",
  component: StorageCleanup,
  beforeEach: () => {
    localStorage.setItem("astro-locale", "zh");
    const state = window as unknown as { storageCleanupCalls: string[] };
    state.storageCleanupCalls = [];
    mockIPC((command, args) => {
      state.storageCleanupCalls.push(command);
      if (command === "prepare_storage_cleanup")
        return {
          token,
          rootPath: "/Users/demo/.astro",
          expiresAtMs: Date.now() + 300000,
          items: [{ path, bytes: 123, policy: "cache_expired" }],
          totalBytes: 123,
          omittedFiles: true,
        };
      if (command === "execute_storage_cleanup") {
        if (!(args as { confirmed?: boolean })?.confirmed)
          throw new Error("cleanup_confirmation_required");
        return {
          batchId: token,
          recoveryPath: `/Users/demo/.astro/backups/storage-cleanup/${token}`,
          movedFiles: 1,
          movedBytes: 123,
          unverifiedFiles: 0,
          manifestComplete: true,
          outcomes: [{ path, status: "moved" }],
        };
      }
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <main
            style={{
              padding: 24,
              minHeight: "100vh",
              boxSizing: "border-box",
              color: "var(--ink)",
              background: "var(--shell-bg)",
            }}
          >
            <Story />
          </main>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  args: {
    active: true,
    enabled: true,
    rootPath: "/Users/demo/.astro",
    onChanged: () => {},
  },
} satisfies Meta<typeof StorageCleanup>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Confirmation: Story = {};
