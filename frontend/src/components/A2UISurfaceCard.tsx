/** 可折叠的 A2UI surface 卡头：非 HITL 的 present_* 工具结果用此包裹。 */

import { useState } from "react";
import { AlertCircle, BarChart3, CheckCircle, ChevronDown, Info } from "lucide-react";
import type { UiSurface } from "../types";
import { collectComponents, parseOperations } from "../a2ui/validate";
import A2UIRenderer from "../a2ui/A2UIRenderer";

type Props = {
  surface: UiSurface;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

type SurfaceKind = "metrics" | "callout" | "result" | "info";

function detectKind(surface: UiSurface): SurfaceKind {
  const ops = parseOperations(surface.operations);
  const components = collectComponents(ops);
  const types = new Set(components.map((c) => c.component));
  if (types.has("Metric")) return "metrics";
  if (types.has("Callout")) return "callout";
  if (types.has("Badge")) return "result";
  return "info";
}

function extractTitle(surface: UiSurface): string {
  const ops = parseOperations(surface.operations);
  const components = collectComponents(ops);
  const titleComp = components.find(
    (c) => c.id === "title" && c.component === "Text",
  );
  if (titleComp && typeof titleComp.text === "string" && titleComp.text) {
    return titleComp.text;
  }
  return surface.activityType ?? "Surface";
}

function KindIcon({ kind }: { kind: SurfaceKind }) {
  switch (kind) {
    case "metrics":
      return <BarChart3 size={14} strokeWidth={2} aria-hidden />;
    case "callout":
      return <AlertCircle size={14} strokeWidth={2} aria-hidden />;
    case "result":
      return <CheckCircle size={14} strokeWidth={2} aria-hidden />;
    default:
      return <Info size={14} strokeWidth={2} aria-hidden />;
  }
}

export default function A2UISurfaceCard({ surface, onAction }: Props) {
  const [open, setOpen] = useState(true);
  const kind = detectKind(surface);
  const title = extractTitle(surface);
  const disabled = surface.status !== "active";

  return (
    <div className={`msg-activity a2ui-surface-card ${open ? "is-open" : ""}`.trim()}>
      <div className="msg-activity-body">
        <button
          type="button"
          className="msg-activity-toggle"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          <span className="msg-activity-kind-icon">
            <KindIcon kind={kind} />
          </span>
          <span className="msg-activity-title">{title}</span>
          <ChevronDown
            size={14}
            strokeWidth={2}
            className="msg-activity-chevron"
            aria-hidden
          />
        </button>
        {open && (
          <div className="a2ui-surface-card-body">
            <A2UIRenderer
              operations={surface.operations}
              disabled={disabled}
              onAction={onAction}
            />
          </div>
        )}
      </div>
    </div>
  );
}
