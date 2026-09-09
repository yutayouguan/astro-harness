import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { McpServerCard } from "../components/settings/McpSection";
import {
  normalizeMcpRuntimeStatus,
  parseMcpJson,
  type McpRuntimeState,
} from "../hooks/providers/useMcpTools";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";

function Fixture({
  enabled = false,
  state,
  list = false,
  pending = false,
  longContent = false,
  withTools = false,
}: {
  enabled?: boolean;
  state?: McpRuntimeState;
  list?: boolean;
  pending?: boolean;
  longContent?: boolean;
  withTools?: boolean;
}) {
  const [reconnecting, setReconnecting] = useState(pending);
  const [server] = parseMcpJson(
    JSON.stringify({
      mcpServers: {
        fixture: {
          command: "/usr/bin/true",
          enabled,
          name: "Config QA",
          default_tools_approval_mode: "prompt",
        },
      },
    }),
  );
  return (
    <div
      className={`mcp-server-grid${list ? " is-list" : ""}`}
      style={{ gridTemplateColumns: "minmax(0, 1fr)" }}
    >
      <McpServerCard
        server={{
          ...server,
          enabled,
          name: longContent
            ? "Research Workspace — 一个很长的服务器名称，用于验证窄屏布局"
            : "Config QA",
          command: longContent
            ? "/workspace/team/research/integrations/documentation/mcp-server --namespace=engineering-knowledge-base"
            : server.command,
          cwd: longContent
            ? "/workspace/team/research/very-long-project-directory/engineering-documentation"
            : undefined,
          envVars: longContent
            ? [
                "DOCUMENTATION_WORKSPACE_SERVICE_TOKEN",
                "RESEARCH_SERVER_CONFIG_PATH",
              ]
            : [],
          discovered: withTools
            ? [
                {
                  name: "search_documents_in_engineering_knowledge_base",
                  description: "Search documents by title and content.",
                  readOnlyHint: true,
                },
                {
                  name: "read_document",
                  description: "Read a selected document.",
                  readOnlyHint: true,
                },
                {
                  name: "update_document",
                  description: "Update document content.",
                  destructiveHint: true,
                },
              ]
            : [],
        }}
        runtimeStatus={
          state
            ? normalizeMcpRuntimeStatus({ id: server.id, status: state })
            : undefined
        }
        reconnecting={reconnecting}
        onReconnect={() => setReconnecting(true)}
        onRemove={() => {}}
        onToggle={() => {}}
        onToggleTool={() => {}}
        onSetServerApprovalMode={() => {}}
        onSetToolApprovalMode={() => {}}
        onRefresh={() => {}}
        onAuthenticate={() => {}}
        onLogout={() => {}}
      />
    </div>
  );
}

const meta = {
  title: "Settings/McpServerCard",
  component: Fixture,
  decorators: [
    (Story) => (
      <LocaleProvider>
        <MorphiconProvider>
          <main
            className="skills-page settings-content-inline"
            data-tone="indigo"
            style={{
              padding: 24,
              width: "100%",
              maxWidth: 760,
              minHeight: "100vh",
              boxSizing: "border-box",
              background: "var(--shell-bg)",
            }}
          >
            <Story />
          </main>
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof Fixture>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Disabled: Story = { args: { enabled: false } };
export const Connected: Story = { args: { enabled: true, state: "connected" } };
export const CompactList: Story = { args: { enabled: false, list: true } };
export const Reconnecting: Story = {
  args: { enabled: true, state: "error", pending: true },
};
export const LongContent: Story = {
  args: {
    enabled: true,
    state: "connected",
    longContent: true,
    withTools: true,
  },
};
export const LongContentList: Story = {
  args: {
    enabled: true,
    state: "connected",
    longContent: true,
    withTools: true,
    list: true,
  },
};
export const AllStates: Story = {
  render: () => (
    <div style={{ display: "grid", gap: 20 }}>
      {(
        [
          "configured",
          "disabled",
          "connecting",
          "connected",
          "disconnected",
          "backoff",
          "auth-required",
          "error",
          "unknown",
        ] as McpRuntimeState[]
      ).map((state) => (
        <section key={state} data-fixture-state={state}>
          <Fixture enabled={state !== "disabled"} state={state} />
        </section>
      ))}
    </div>
  ),
};
