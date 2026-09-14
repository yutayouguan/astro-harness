import type { MessageKey } from "../../i18n/messages.ts";

export type Translate = (
  key: MessageKey,
  vars?: Record<string, string>,
) => string;

export type AgentLogLine = {
  raw: string;
  source: string;
  timestamp: string;
  level: string;
  message: string;
};

export type DiagnosticsStatusDto = {
  backendHealthy: boolean;
  backendEndpoint: string;
  backendError: string | null;
  providerEnabled: number;
  providerTotal: number;
  activeProviderId: string | null;
  activeProviderName: string | null;
  providerError: string | null;
  mcpConnected: number;
  mcpTotal: number;
  mcpRetrying: number;
  mcpError: string | null;
  databaseHealthy: boolean;
  databaseJournalMode: string;
  databaseSchemaVersion: number | null;
  databaseError: string | null;
};

export type LogSourceFilter = "both" | "agent" | "errors";
export type LogScope = "current" | "all";
export type LogLevelFilter = "all" | "issues";
export type DiagnosticLogLevel =
  | "error"
  | "warn"
  | "info"
  | "debug"
  | "unknown";

export type DiagnosticStatusCardModel = {
  id: string;
  label: string;
  value: string;
  detail: string;
  state: "healthy" | "configured" | "warning" | "error" | "unknown";
};

export function diagnosticLogLevel(raw: string): DiagnosticLogLevel {
  const upper = raw.toUpperCase();
  if (upper.includes("CRITICAL") || upper.includes("ERROR")) return "error";
  if (upper.includes("WARNING") || upper.includes("WARN")) return "warn";
  if (upper.includes("INFO")) return "info";
  if (upper.includes("DEBUG") || upper.includes("TRACE")) return "debug";
  return "unknown";
}

export function buildDiagnosticStatusCards(
  status: DiagnosticsStatusDto | null,
  busy: boolean,
  error: string,
  t: Translate,
): DiagnosticStatusCardModel[] {
  if (!status) {
    return (["backend", "provider", "mcp", "database"] as const).map((id) => ({
      id,
      label: t(`prefs.diag.status.${id}` as MessageKey),
      value: busy ? t("prefs.diag.status.checking") : "—",
      detail: error
        ? t("prefs.diag.status.unavailable")
        : t("prefs.diag.status.waiting"),
      state: "unknown" as const,
    }));
  }

  return [
    {
      id: "backend",
      label: t("prefs.diag.status.backend"),
      value: t(
        status.backendHealthy
          ? "prefs.diag.status.healthy"
          : "prefs.diag.status.unavailable",
      ),
      detail: status.backendHealthy
        ? t("prefs.diag.status.backendDetail", {
            endpoint: status.backendEndpoint.replace(/^https?:\/\//, ""),
          })
        : t("prefs.diag.status.unavailable"),
      state: status.backendHealthy ? "healthy" : "error",
    },
    {
      id: "provider",
      label: t("prefs.diag.status.provider"),
      value: status.providerError
        ? t("prefs.diag.status.unavailable")
        : t("prefs.diag.status.providerEnabled", {
            count: String(status.providerEnabled),
            total: String(status.providerTotal),
          }),
      detail: status.providerError
        ? t("prefs.diag.status.unavailable")
        : status.activeProviderName || status.activeProviderId
          ? t("prefs.diag.status.providerConfigured", {
              provider:
                status.activeProviderName ?? status.activeProviderId ?? "",
            })
          : t("prefs.diag.status.noneActive"),
      state: status.providerError
        ? "error"
        : status.providerEnabled > 0
          ? "configured"
          : "warning",
    },
    {
      id: "mcp",
      label: t("prefs.diag.status.mcp"),
      value: status.mcpError
        ? t("prefs.diag.status.unavailable")
        : status.mcpTotal === 0
          ? t("prefs.diag.status.notConfigured")
          : t("prefs.diag.status.mcpConnected", {
              count: String(status.mcpConnected),
              total: String(status.mcpTotal),
            }),
      detail: status.mcpError
        ? t("prefs.diag.status.unavailable")
        : status.mcpTotal === 0
          ? t("prefs.diag.status.mcpNone")
          : status.mcpRetrying > 0
            ? t("prefs.diag.status.mcpRetrying", {
                count: String(status.mcpRetrying),
              })
            : status.mcpConnected === status.mcpTotal
              ? t("prefs.diag.status.mcpReady")
              : t("prefs.diag.status.mcpDisconnected", {
                  count: String(status.mcpTotal - status.mcpConnected),
                }),
      state: status.mcpError
        ? "error"
        : status.mcpTotal === 0
          ? "unknown"
          : status.mcpRetrying > 0 || status.mcpConnected < status.mcpTotal
            ? "warning"
            : "healthy",
    },
    {
      id: "database",
      label: t("prefs.diag.status.database"),
      value: t(
        status.databaseHealthy
          ? "prefs.diag.status.databaseReadable"
          : "prefs.diag.status.unavailable",
      ),
      detail:
        status.databaseHealthy && status.databaseSchemaVersion != null
          ? t("prefs.diag.status.databaseInspection", {
              version: String(status.databaseSchemaVersion),
              mode: status.databaseJournalMode,
            })
          : t("prefs.diag.status.unavailable"),
      state: status.databaseHealthy ? "healthy" : "error",
    },
  ];
}
