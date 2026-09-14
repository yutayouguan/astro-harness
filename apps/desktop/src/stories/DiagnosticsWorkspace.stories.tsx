import type { Meta, StoryObj } from "@storybook/react-vite";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useEffect, useState } from "react";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import preferencesMeta from "./PreferencesPanel.stories";
import storageMeta from "./StorageDiagnostics.stories";
import type { AgentLogLine } from "../lib/settings/diagnosticsModel";

function Preview() {
  const { mode, setMode, material, setMaterial } = useTheme();
  const [active, setActive] = useState(true);
  useEffect(() => {
    setMode("dark");
    setMaterial("soft");
  }, [setMode, setMaterial]);
  return (
    <main
      className="app-shell settings-content-inline"
      data-tone="orange"
      style={{ padding: 20 }}
    >
      <nav
        aria-label="诊断测试样板"
        style={{
          display: "flex",
          gap: 12,
          paddingBottom: 12,
          flexShrink: 0,
          fontSize: 12,
        }}
      >
        <button
          type="button"
          onClick={() => setMode(mode === "dark" ? "light" : "dark")}
        >
          切换明暗
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
        <button type="button" onClick={() => setActive(!active)}>
          {active ? "离开诊断" : "返回诊断"}
        </button>
        <span>隔离样板：模拟数据，不访问真实日志或文件。</span>
      </nav>
      <PreferencesPanel
        {...preferencesMeta.args}
        mode={mode}
        onChange={setMode}
        section={active ? "diagnostics" : "general"}
        activeSessionId="diagnostics-demo"
      />
    </main>
  );
}

const meta = {
  title: "Settings/Diagnostics Workspace",
  component: Preview,
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <MorphiconProvider>
            <Story />
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  beforeEach: () => {
    localStorage.setItem("astro-locale", "zh");
    const rows: AgentLogLine[] = Array.from({ length: 48 }, (_, index) => {
      const level = index === 2 ? "ERROR" : index % 7 === 0 ? "WARN" : "INFO";
      const message =
        index === 2
          ? "模型请求超时，已排队重试。\\nprovider=demo · timeout=30s"
          : index % 7 === 0
            ? "MCP 服务连接中，等待下一次重试"
            : [
                "任务已完成，历史记录已保存",
                "读取全局配置成功",
                "工具执行完成 · elapsed=126ms",
                "后台索引更新完成",
              ][index % 4];
      return {
        timestamp: new Date(Date.now() - index * 4300).toISOString(),
        level,
        source: level === "ERROR" ? "errors" : "agent",
        message,
        raw: `[${level}] session_id=diagnostics-demo turn_id=turn-demo ${message}`,
      };
    });
    mockIPC((command, payload) => {
      if (command === "query_agent_logs") {
        const args = (
          payload as {
            args: {
              source: string;
              minLevel: string | null;
              lines: number;
              sinceMs: number | null;
              untilMs: number | null;
              sessionId: string | null;
              turnId: string | null;
            };
          }
        ).args;
        return rows
          .filter(
            (row) =>
              (args.source === "both" || row.source === args.source) &&
              (!args.minLevel || row.level !== "INFO") &&
              (args.sinceMs == null ||
                Date.parse(row.timestamp) >= args.sinceMs) &&
              (args.untilMs == null ||
                Date.parse(row.timestamp) <= args.untilMs) &&
              (!args.sessionId ||
                row.raw.includes(`session_id=${args.sessionId}`)) &&
              (!args.turnId || row.raw.includes(`turn_id=${args.turnId}`)),
          )
          .slice(0, args.lines);
      }
      if (command === "get_diagnostics_status")
        return {
          backendHealthy: true,
          backendEndpoint: "http://127.0.0.1:64884",
          backendError: null,
          providerEnabled: 1,
          providerTotal: 1,
          activeProviderId: "demo",
          activeProviderName: "Azure OpenAI",
          providerError: null,
          mcpConnected: 0,
          mcpTotal: 0,
          mcpRetrying: 0,
          mcpError: null,
          databaseHealthy: true,
          databaseJournalMode: "WAL",
          databaseSchemaVersion: 23,
          databaseError: null,
        };
      if (command === "inspect_home_storage")
        return storageMeta.args.initialReport;
      if (command === "export_diagnostics_bundle")
        return "/preview/astro-diagnostics.zip";
      if (command === "get_app_icon") return { current: "blue", options: [] };
      if (command === "get_compression_settings") return null;
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      if (command === "discard_storage_cleanup") return null;
      if (command === "prepare_storage_cleanup")
        return {
          token: "00000000-0000-4000-8000-000000000001",
          rootPath: "/Users/demo/.astro",
          expiresAtMs: Date.now() + 300_000,
          items: [
            {
              path: "models/cache/old-model-metadata.json",
              bytes: 131072,
              policy: "cache_expired",
            },
          ],
          totalBytes: 131072,
          omittedFiles: false,
        };
      // Execution is unavailable: this fixture can only inspect/prepare/cancel.
      throw new Error(`Read-only diagnostics fixture: ${command}`);
    });
    return clearMocks;
  },
} satisfies Meta<typeof Preview>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Workbench: Story = {};
