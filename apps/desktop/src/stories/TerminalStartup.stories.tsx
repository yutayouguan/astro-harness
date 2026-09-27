import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { Terminal } from "@xterm/xterm";
import type { Meta, StoryObj } from "@storybook/react-vite";
import TerminalTabsDock from "../components/chat/TerminalTabsDock";
import { LocaleProvider } from "../i18n/LocaleContext";

// Captured from a real zsh -f PTY: PROMPT_SP pads to the PTY width and then
// erases the marker before printing the first prompt. Replaying at another
// width leaves an inverse '%' (or blank lines) above the prompt.
const startup = (cols: number) =>
  `\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m${" ".repeat(cols - 1)}\r \r\r\x1b[0m\x1b[27m\x1b[24m\x1b[JAI workspace % \x1b[K\x1b[?2004h`;

const meta = {
  title: "Chat/TerminalStartup",
  component: TerminalTabsDock,
  beforeEach: (context) => {
    const chunkSize = context.id.endsWith("--split-output")
      ? 7
      : Number.POSITIVE_INFINITY;
    let terminal: Terminal | undefined;
    let nextId = 1;
    const sessions = new Map<number, number[]>();
    const requests: {
      command: string;
      cols?: number;
      rows?: number;
      initialInput?: string;
    }[] = [];
    const originalOpen = Terminal.prototype.open;
    Terminal.prototype.open = function (parent: HTMLElement) {
      terminal = this;
      return originalOpen.call(this, parent);
    };
    const preview = {
      requests,
      screen: () => ({
        cols: terminal?.cols,
        cursorY: terminal?.buffer.active.cursorY,
        lines: Array.from({ length: 4 }, (_, i) =>
          terminal?.buffer.active.getLine(i)?.translateToString(true),
        ),
      }),
    };
    Object.assign(window, { terminalStartupPreview: preview });
    mockIPC((command, payload) => {
      const request = (
        payload as {
          request?: {
            id: number;
            cols: number;
            rows: number;
            cursor: number;
            scope: string;
            cwd: string;
            initialInput?: string;
          };
        }
      )?.request;
      if (command === "set_app_menu_locale") return null;
      if (command === "terminal_open") {
        requests.push({
          command,
          cols: request!.cols,
          rows: request!.rows,
          initialInput: request!.initialInput,
        });
        const id = nextId++;
        const data = Array.from(
          new TextEncoder().encode(startup(request!.cols)),
        );
        sessions.set(id, data);
        return {
          id,
          scope: request!.scope,
          cwd: request!.cwd,
          running: true,
          baseCursor: 0,
          endCursor: data.length,
        };
      }
      if (command === "terminal_resize") {
        requests.push({ command, cols: request!.cols, rows: request!.rows });
        return null;
      }
      if (command === "terminal_read") {
        requests.push({ command });
        const data = sessions.get(request!.id)!;
        if (request!.cursor < data.length) {
          const nextCursor = Math.min(data.length, request!.cursor + chunkSize);
          return {
            id: request!.id,
            data: data.slice(request!.cursor, nextCursor),
            nextCursor,
            dropped: false,
            running: true,
          };
        }
        // An idle PTY is a long poll, not a busy loop.
        return new Promise(() => {});
      }
      if (command === "terminal_write" || command === "terminal_close")
        return null;
      throw new Error(`Unexpected terminal fixture command: ${command}`);
    });
    return () => {
      clearMocks();
      Terminal.prototype.open = originalOpen;
      Reflect.deleteProperty(window, "terminalStartupPreview");
    };
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            height: "100vh",
            background: "var(--bg)",
          }}
        >
          <Story />
        </div>
      </LocaleProvider>
    ),
  ],
  args: {
    open: true,
    projectId: "terminal-startup-fixture",
    projectName: "Fixture",
    projectRoot: "/workspace",
    onClose: () => {},
  },
} satisfies Meta<typeof TerminalTabsDock>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Narrow: Story = {};
export const SplitOutput: Story = {};
// 审批卡「在终端打开」：dock 收到外部 prefill 请求时新开一个预填命令的标签页。
export const Prefilled: Story = {
  args: {
    prefill: { token: 1, command: "cat README.md" },
  },
};
