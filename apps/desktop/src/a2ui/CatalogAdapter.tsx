/** CatalogAdapter：将 A2UI 组件映射为 React 节点。 */

import type { LucideIcon } from "lucide-react";
import {
  AlertTriangle,
  Check,
  Clapperboard,
  HelpCircle,
  Image as ImageIcon,
  Info,
  MapPin,
  Music2,
  Shield,
} from "lucide-react";
import type { ReactNode } from "react";
import GeneratedMediaCard from "../components/media/GeneratedMediaCard";
import MediaPreview from "../components/media/MediaPreview";
import MediaToolbar from "../components/media/MediaToolbar";
import { resolveMediaPreviewPath } from "../lib/media/resolveMediaSrc";
import ClarifyWizard from "./ClarifyWizard";
import { parseClarifySteps } from "./clarifySteps";
import { mergeActionContext, missingRequiredFields } from "./formState";
import type { A2uiComponent } from "./types";
import { isKnownComponent } from "./validate";

const AVATAR_ICONS: Record<string, LucideIcon> = {
  shield: Shield,
  info: Info,
  help: HelpCircle,
  warn: AlertTriangle,
  warning: AlertTriangle,
  check: Check,
  success: Check,
  "map-pin": MapPin,
  music: Music2,
  clapperboard: Clapperboard,
  image: ImageIcon,
};

type RenderCtx = {
  byId: Map<string, A2uiComponent>;
  disabled: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
  unknownLabel: string;
  fieldValues: Record<string, unknown>;
  setFieldValue: (id: string, value: unknown) => void;
  mediaBaseDir?: string | null;
  /** True when any required field is still empty. */
  requiredBlocked: boolean;
};

function renderChild(
  id: string | undefined,
  ctx: RenderCtx,
  key?: string,
): ReactNode {
  if (!id) return null;
  const node = ctx.byId.get(id);
  if (!node) return null;
  return <CatalogNode key={key ?? id} node={node} ctx={ctx} />;
}

function CatalogNode({
  node,
  ctx,
}: {
  node: A2uiComponent;
  ctx: RenderCtx;
}) {
  if (!isKnownComponent(node.component)) {
    return (
      <div className="a2ui-unknown" data-component={node.component}>
        {ctx.unknownLabel}: {node.component}
      </div>
    );
  }

  switch (node.component) {
    case "Text": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = node.variant ?? "body";
      if (variant === "h1")
        return (
          <h1 className="a2ui-text a2ui-h1" data-a2ui-id={node.id}>
            {text}
          </h1>
        );
      if (variant === "h2")
        return (
          <h2 className="a2ui-text a2ui-h2" data-a2ui-id={node.id}>
            {text}
          </h2>
        );
      if (variant === "caption")
        return (
          <p className="a2ui-text a2ui-caption" data-a2ui-id={node.id}>
            {text}
          </p>
        );
      return (
        <p className="a2ui-text" data-a2ui-id={node.id}>
          {text}
        </p>
      );
    }
    case "Icon": {
      const name = typeof node.name === "string" ? node.name : "";
      const Icon = name ? AVATAR_ICONS[name.toLowerCase()] : undefined;
      return (
        <span className="a2ui-icon" aria-hidden>
          {Icon ? <Icon size={16} strokeWidth={2} /> : name || "•"}
        </span>
      );
    }
    case "Divider":
      return <hr className="a2ui-divider" />;
    case "Card": {
      const mediaVariant = node.variant === "media";
      return (
        <div
          className={`a2ui-card ${mediaVariant ? "is-media" : ""}`.trim()}
          data-a2ui-id={node.id}
        >
          {renderChild(node.child, ctx)}
        </div>
      );
    }
    case "Column":
      return (
        <div className="a2ui-column">
          {(node.children ?? []).map((id) => renderChild(id, ctx, id))}
        </div>
      );
    case "Row":
      return (
        <div className="a2ui-row" data-a2ui-id={node.id}>
          {(node.children ?? []).map((id) => renderChild(id, ctx, id))}
        </div>
      );
    case "List":
      return (
        <ul className="a2ui-list">
          {(node.children ?? []).map((id) => (
            <li key={id}>{renderChild(id, ctx)}</li>
          ))}
        </ul>
      );
    case "Image":
    case "Audio":
    case "Video": {
      const src =
        (typeof node.src === "string" && node.src) ||
        (typeof node.url === "string" && node.url) ||
        "";
      if (!src) return null;
      const path = resolveMediaPreviewPath(src, ctx.mediaBaseDir);
      const kind =
        node.component === "Audio"
          ? "audio"
          : node.component === "Video"
            ? "video"
            : "image";
      const alt = typeof node.alt === "string" ? node.alt : undefined;
      // 媒体 surface（Card variant=media）已有标题头：槽内只放播放器，避免双层卡头
      const nestedInMediaCard = Boolean(
        [...ctx.byId.values()].some(
          (c) => c.component === "Card" && c.variant === "media",
        ),
      );
      if (nestedInMediaCard) {
        return (
          <div
            className={`a2ui-media-slot a2ui-media-${kind}`}
            data-a2ui-id={node.id}
          >
            <div className="a2ui-media-slot-actions">
              <MediaToolbar
                path={path}
                kind={kind}
                compact
                className="is-inline"
                alt={alt}
              />
            </div>
            <MediaPreview
              kind={kind}
              path={path}
              alt={alt}
              compact
              showToolbar={false}
              className="a2ui-media-preview"
            />
          </div>
        );
      }
      return (
        <div
          className="a2ui-image-wrap a2ui-image a2ui-image-hero"
          data-a2ui-id={node.id}
        >
          <GeneratedMediaCard kind={kind} path={path} label={alt} compact />
        </div>
      );
    }
    case "Button": {
      const eventName = node.action?.event?.name ?? "click";
      const context = node.action?.event?.context ?? {};
      const blocked = ctx.disabled || ctx.requiredBlocked;
      return (
        <button
          type="button"
          className={`a2ui-button ${node.variant === "primary" ? "is-primary" : ""}`}
          disabled={blocked}
          onClick={() =>
            ctx.onAction(
              eventName,
              mergeActionContext(context, ctx.fieldValues),
            )
          }
        >
          {renderChild(node.child, ctx)}
        </button>
      );
    }
    case "TextField": {
      const label = typeof node.label === "string" ? node.label : typeof node.text === "string" ? node.text : "";
      const current =
        ctx.fieldValues[node.id] != null
          ? String(ctx.fieldValues[node.id])
          : typeof node.value === "string"
            ? node.value
            : "";
      return (
        <label className="a2ui-field">
          {label ? <span className="a2ui-caption">{label}</span> : null}
          <input
            type="text"
            disabled={ctx.disabled}
            value={current}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.value)}
          />
        </label>
      );
    }
    case "ChoicePicker": {
      const label = typeof node.label === "string" ? node.label : "";
      const options = Array.isArray(node.options) ? node.options : [];
      const current =
        ctx.fieldValues[node.id] != null
          ? String(ctx.fieldValues[node.id])
          : typeof node.value === "string"
            ? node.value
            : "";
      return (
        <label className="a2ui-choice">
          {label ? <span className="a2ui-caption">{label}</span> : null}
          <select
            disabled={ctx.disabled}
            value={current}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.value)}
          >
            <option value="">—</option>
            {options.map((opt, i) => {
              if (typeof opt === "string") {
                return (
                  <option key={i} value={opt}>
                    {opt}
                  </option>
                );
              }
              const v = opt.value ?? opt.label ?? "";
              return (
                <option key={i} value={v}>
                  {opt.label ?? v}
                </option>
              );
            })}
          </select>
        </label>
      );
    }
    case "CheckBox": {
      const label = typeof node.label === "string" ? node.label : typeof node.text === "string" ? node.text : "";
      const checked =
        typeof ctx.fieldValues[node.id] === "boolean"
          ? Boolean(ctx.fieldValues[node.id])
          : Boolean(node.value);
      return (
        <label className="a2ui-check">
          <input
            type="checkbox"
            disabled={ctx.disabled}
            checked={checked}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.checked)}
          />
          <span>{label}</span>
        </label>
      );
    }
    case "Badge": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = typeof node.variant === "string" ? node.variant : "info";
      return (
        <span
          className={`a2ui-badge is-${variant}`}
          data-a2ui-id={node.id}
        >
          {text}
        </span>
      );
    }
    case "Chip": {
      const text = typeof node.text === "string" ? node.text : "";
      const eventName = node.action?.event?.name;
      if (eventName) {
        const blocked = ctx.disabled || ctx.requiredBlocked;
        return (
          <button
            type="button"
            className="a2ui-chip"
            disabled={blocked}
            onClick={() =>
              ctx.onAction(
                eventName,
                mergeActionContext(
                  node.action?.event?.context ?? {},
                  ctx.fieldValues,
                ),
              )
            }
          >
            {text}
          </button>
        );
      }
      return <span className="a2ui-chip">{text}</span>;
    }
    case "Metric": {
      const label = typeof node.label === "string" ? node.label : "";
      const value = node.value != null ? String(node.value) : "";
      const hint = typeof node.hint === "string" ? node.hint : "";
      return (
        <div className="a2ui-metric">
          <div className="a2ui-metric-label">{label}</div>
          <div className="a2ui-metric-value">{value}</div>
          {hint ? <div className="a2ui-metric-hint">{hint}</div> : null}
        </div>
      );
    }
    case "Avatar": {
      const src =
        (typeof node.src === "string" && node.src) ||
        (typeof node.url === "string" && node.url) ||
        "";
      const text = typeof node.text === "string" ? node.text : "";
      const name = typeof node.name === "string" ? node.name : "";
      const Icon = name ? AVATAR_ICONS[name.toLowerCase()] : undefined;
      return (
        <div className="a2ui-avatar" aria-hidden>
          {src ? (
            <img src={src} alt="" />
          ) : Icon ? (
            <Icon size={18} strokeWidth={2} />
          ) : (
            text || name || "•"
          )}
        </div>
      );
    }
    case "Callout": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = node.variant === "warn" ? "warn" : "info";
      return (
        <div className={`a2ui-callout is-${variant}`}>{text}</div>
      );
    }
    case "Spacer": {
      const size =
        node.size === "sm" || node.size === "lg" ? node.size : "md";
      return <div className={`a2ui-spacer-${size}`} />;
    }
    case "ClarifyWizard": {
      const steps = parseClarifySteps(node.steps);
      if (!steps.length) return null;
      return (
        <ClarifyWizard
          steps={steps}
          variant={node.variant === "approval" ? "approval" : "default"}
          approvalTitle={typeof node.title === "string" ? node.title : undefined}
          approvalBody={typeof node.body === "string" ? node.body : undefined}
          allowAlways={node.allowAlways === true}
          disabled={ctx.disabled}
          onAction={ctx.onAction}
        />
      );
    }
    default:
      return null;
  }
}

export function renderCatalogTree(
  components: A2uiComponent[],
  opts: {
    disabled: boolean;
    onAction: (name: string, context: Record<string, unknown>) => void;
    unknownLabel: string;
    fieldValues: Record<string, unknown>;
    setFieldValue: (id: string, value: unknown) => void;
    mediaBaseDir?: string | null;
  },
): ReactNode {
  const byId = new Map(components.map((c) => [c.id, c]));
  const referenced = new Set<string>();
  for (const c of components) {
    if (c.child) referenced.add(c.child);
    for (const id of c.children ?? []) referenced.add(id);
  }
  const roots = components.filter((c) => !referenced.has(c.id));
  const root = roots.find((c) => c.component === "Card") ?? roots[0] ?? components[0];
  if (!root) return null;
  const requiredBlocked =
    missingRequiredFields(components, opts.fieldValues).length > 0;
  const ctx: RenderCtx = {
    byId,
    disabled: opts.disabled,
    onAction: opts.onAction,
    unknownLabel: opts.unknownLabel,
    fieldValues: opts.fieldValues,
    setFieldValue: opts.setFieldValue,
    mediaBaseDir: opts.mediaBaseDir,
    requiredBlocked,
  };
  return <CatalogNode node={root} ctx={ctx} />;
}
