import type { Meta, StoryObj } from "@storybook/react-vite";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import ProjectEditDialog from "../components/chat/ProjectEditDialog";
import { ThemeProvider } from "../hooks/app/useTheme";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ProjectDto } from "../types";

/** 与真实 APP 一致：ThemeProvider 安装输入焦点环运行时，组合字段由外壳承载焦点。 */
const project: ProjectDto = {
  id: "astro-demo",
  name: "Astro",
  icon: "folder-project",
  roots: ["/Users/demo/Desktop/astro", "/Users/demo/Documents/notes"],
  position: 0,
  createdAt: "2026-09-01T00:00:00Z",
  updatedAt: "2026-09-01T00:00:00Z",
};

const meta = {
  title: "Design/ProjectDialogFocus",
  component: ProjectEditDialog,
  parameters: { layout: "fullscreen" },
  args: {
    open: true,
    project,
    onClose: () => {},
    onUpdated: () => {},
    onCreated: () => {},
    onRemoved: () => {},
  },
  beforeEach: () => {
    localStorage.setItem("astro-locale", "zh");
    mockIPC(() => null);
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <DialogProvider>
            <main
              style={{
                boxSizing: "border-box",
                minHeight: "100vh",
                padding: 24,
                background: "var(--shell-bg)",
                color: "var(--ink)",
              }}
            >
              <Story />
            </main>
          </DialogProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
} satisfies Meta<typeof ProjectEditDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

/** 名称行是组合字段：焦点环归外壳，内部裸 input 不得再画一圈方框。 */
export const NameFieldFocus: Story = {};
