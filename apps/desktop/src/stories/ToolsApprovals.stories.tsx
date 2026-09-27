import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import ToolsPanel from "../components/settings/ToolsPanel";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";

const approvalSettings = {
  preset: "approve_for_me",
  browserApprovalRules: [],
  commandTypeAllowlist: [],
  commandAllowlist: [],
};

let writeRoots = ["/tmp/qa-shared-out"];

const meta = {
  id: "tools-approvals",
  title: "Settings/Tools Approvals",
  component: ToolsPanel,
  beforeEach: () => {
    writeRoots = ["/tmp/qa-shared-out"];
    mockIPC((command, payload) => {
      if (command === "get_approval_settings") return approvalSettings;
      if (command === "get_permission_write_roots") {
        return { profileId: ":workspace", roots: writeRoots };
      }
      if (command === "set_permission_write_roots") {
        writeRoots = (payload as { roots: string[] }).roots;
        return { profileId: ":workspace", roots: writeRoots };
      }
      if (command === "get_session_permission_grants") {
        return { workspaceWrite: true, writableRoots: ["/tmp/qa-session-out"] };
      }
      if (command === "promote_session_write_roots") {
        writeRoots = [...writeRoots, "/tmp/qa-session-out"];
        return { profileId: ":workspace", roots: writeRoots };
      }
      if (command === "get_config") {
        return { active_agent_id: "default", agents: [], workspace_dir: "" };
      }
      if (command === "list_security_audit_page") {
        return { items: [], nextCursor: null };
      }
      if (command === "get_security_audit_retention") {
        return {
          sources: [
            {
              source: "permission",
              maxFileBytes: 10_485_760,
              archiveCount: 4,
              retainedFileCount: 5,
              maxTotalBytes: 52_428_800,
            },
            {
              source: "sandbox",
              maxFileBytes: 10_485_760,
              archiveCount: 4,
              retainedFileCount: 5,
              maxTotalBytes: 52_428_800,
            },
          ],
          maxTotalBytes: 104_857_600,
        };
      }
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <ActiveAgentProvider>
          <MorphiconProvider>
            <DialogProvider>
              <main
                className="settings-content-inline"
                style={{
                  boxSizing: "border-box",
                  width: "100vw",
                  height: "100vh",
                  paddingBlock: 28,
                  background: "var(--shell-bg)",
                  color: "var(--ink)",
                }}
              >
                <Story />
              </main>
            </DialogProvider>
          </MorphiconProvider>
        </ActiveAgentProvider>
      </LocaleProvider>
    ),
  ],
  args: {
    active: true,
    initialTab: "approvals",
    sessionId: "sess-qa-approvals",
  },
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof ToolsPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
