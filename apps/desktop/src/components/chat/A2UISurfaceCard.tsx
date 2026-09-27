/** 可折叠的 A2UI surface 卡头：非 HITL 的 present_* 工具结果用此包裹。 */

import { useState } from "react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import type { UiSurface } from "../../types";
import { collectComponents, parseOperations } from "../../a2ui/validate";
import A2UIRenderer from "../../a2ui/A2UIRenderer";
import { MorphToggleIcon } from "../icons/MorphIcon";

type Props = {
  surface: UiSurface;
  mediaBaseDir?: string | null;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

type SurfaceKind = "metrics" | "callout" | "result" | "info" | "media";

function surfaceComponents(surface: UiSurface) {
  const ops = parseOperations(surface.operations);
  return collectComponents(ops);
}

function detectKind(surface: UiSurface): SurfaceKind {
  const components = surfaceComponents(surface);
  const types = new Set(components.map((c) => c.component));
  if (
    types.has("Audio") ||
    types.has("Video") ||
    (types.has("Image") && types.has("Badge"))
  ) {
    const card = components.find((c) => c.component === "Card");
    if (card?.variant === "media" || types.has("Audio") || types.has("Video")) {
      return "media";
    }
  }
  const badge = components.find((c) => c.component === "Badge");
  if (types.has("Metric")) return "metrics";
  if (types.has("Callout")) return "callout";
  if (badge) {
    const text =
      typeof badge.text === "string" ? badge.text.trim().toLowerCase() : "";
    if (text === "info") return "info";
    return "result";
  }
  return "info";
}

function extractTitle(surface: UiSurface): string {
  const components = surfaceComponents(surface);
  const titleComp = components.find(
    (c) => c.id === "title" && c.component === "Text",
  );
  if (titleComp && typeof titleComp.text === "string" && titleComp.text) {
    return titleComp.text;
  }
  return surface.activityType ?? "Surface";
}

function extractStatusBadge(surface: UiSurface) {
  const components = surfaceComponents(surface);
  const badge = components.find((c) => c.component === "Badge");
  if (!badge || typeof badge.text !== "string" || !badge.text.trim()) {
    return null;
  }
  const text = badge.text.trim().toLowerCase();
  if (text === "info") return null;
  const variant =
    typeof badge.variant === "string" &&
    ["success", "warn", "danger", "info"].includes(badge.variant)
      ? badge.variant
      : "success";
  return { text: badge.text.trim(), variant };
}

export default function A2UISurfaceCard({
  surface,
  mediaBaseDir,
  onAction,
}: Props) {
  const kind = detectKind(surface);
  const [open, setOpen] = useState(true);
  const title = extractTitle(surface);
  const statusBadge = extractStatusBadge(surface);
  const disabled = surface.status !== "active";
  const isMedia = kind === "media";

  return (
    <div
      className={`msg-activity a2ui-surface-card is-kind-${kind} ${open || isMedia ? "is-open" : ""}`.trim()}
    >
      <div className="msg-activity-body">
        {isMedia ? null : (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            <span className="msg-activity-title">{title}</span>
            {statusBadge ? (
              <span
                className={`a2ui-badge is-${statusBadge.variant} a2ui-surface-header-badge`}
              >
                {statusBadge.text}
              </span>
            ) : null}
            <MorphToggleIcon
              active={open}
              activeIcon={ChevronUpData}
              inactiveIcon={ChevronDownData}
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        )}
        {(open || isMedia) && (
          <div className="a2ui-surface-card-body">
            <A2UIRenderer
              operations={surface.operations}
              disabled={disabled}
              mediaBaseDir={mediaBaseDir}
              onAction={onAction}
            />
          </div>
        )}
      </div>
    </div>
  );
}
