/** CatalogAdapter：将 A2UI 组件映射为 React 节点。 */

import type { ReactNode } from "react";
import type { A2uiComponent } from "./types";
import { isKnownComponent } from "./validate";

type RenderCtx = {
  byId: Map<string, A2uiComponent>;
  disabled: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
  unknownLabel: string;
  fieldValues: Record<string, unknown>;
  setFieldValue: (id: string, value: unknown) => void;
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
      if (variant === "h1") return <h1 className="a2ui-text a2ui-h1">{text}</h1>;
      if (variant === "h2") return <h2 className="a2ui-text a2ui-h2">{text}</h2>;
      if (variant === "caption")
        return <p className="a2ui-text a2ui-caption">{text}</p>;
      return <p className="a2ui-text">{text}</p>;
    }
    case "Icon":
      return (
        <span className="a2ui-icon" aria-hidden>
          {typeof node.name === "string" ? node.name : "•"}
        </span>
      );
    case "Divider":
      return <hr className="a2ui-divider" />;
    case "Card":
      return (
        <div className="a2ui-card">{renderChild(node.child, ctx)}</div>
      );
    case "Column":
      return (
        <div className="a2ui-column">
          {(node.children ?? []).map((id) => renderChild(id, ctx, id))}
        </div>
      );
    case "Row":
      return (
        <div className="a2ui-row">
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
    case "Image": {
      const src =
        (typeof node.src === "string" && node.src) ||
        (typeof node.url === "string" && node.url) ||
        "";
      if (!src) return null;
      return <img className="a2ui-image" src={src} alt="" />;
    }
    case "Button": {
      const eventName = node.action?.event?.name ?? "click";
      const context = node.action?.event?.context ?? {};
      return (
        <button
          type="button"
          className={`a2ui-button ${node.variant === "primary" ? "is-primary" : ""}`}
          disabled={ctx.disabled}
          onClick={() => ctx.onAction(eventName, context)}
        >
          {renderChild(node.child, ctx)}
        </button>
      );
    }
    case "TextField":
    case "ChoicePicker":
    case "CheckBox":
      // MVP：HITL 澄清用 Button 选项；表单控件占位
      return (
        <div className="a2ui-field-placeholder" data-component={node.component}>
          {typeof node.text === "string" ? node.text : node.component}
        </div>
      );
    case "Badge": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = typeof node.variant === "string" ? node.variant : "info";
      return (
        <span className={`a2ui-badge is-${variant}`}>{text}</span>
      );
    }
    case "Chip": {
      const text = typeof node.text === "string" ? node.text : "";
      const eventName = node.action?.event?.name;
      if (eventName) {
        return (
          <button
            type="button"
            className="a2ui-chip"
            disabled={ctx.disabled}
            onClick={() =>
              ctx.onAction(eventName, node.action?.event?.context ?? {})
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
      return (
        <div className="a2ui-avatar" aria-hidden>
          {src ? <img src={src} alt="" /> : text || name || "•"}
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
  const ctx: RenderCtx = {
    byId,
    disabled: opts.disabled,
    onAction: opts.onAction,
    unknownLabel: opts.unknownLabel,
    fieldValues: {},
    setFieldValue: () => {},
  };
  return <CatalogNode node={root} ctx={ctx} />;
}
