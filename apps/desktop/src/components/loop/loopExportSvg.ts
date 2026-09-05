export interface WorkflowSvgNode {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  label: string;
  subtitle: string;
  color: string;
  disabled?: boolean;
}

export interface WorkflowSvgEdge {
  source: string;
  target: string;
  type?: string;
  label?: string;
}

interface BuildWorkflowSvgOptions {
  title: string;
  summary: string;
  nodes: WorkflowSvgNode[];
  edges: WorkflowSvgEdge[];
  dark: boolean;
}

const PADDING_X = 48;
const PADDING_BOTTOM = 44;
const HEADER_HEIGHT = 58;

function escapeXml(value: string): string {
  return value.replace(/[&<>"']/g, (char) => {
    switch (char) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&apos;";
    }
  });
}

function safeColor(value: string): string {
  return /^(?:#[\da-f]{3,8}|rgb(?:a)?\([\d\s.,%]+\)|hsl(?:a)?\([\d\s.,%]+\))$/i.test(
    value.trim(),
  )
    ? value.trim()
    : "#8b5cf6";
}

function truncate(value: string, max: number): string {
  const chars = Array.from(value);
  return chars.length > max ? `${chars.slice(0, max - 1).join("")}…` : value;
}

function edgePath(
  sx: number,
  sy: number,
  tx: number,
  ty: number,
  type: string | undefined,
): string {
  if (type === "straight") return `M ${sx} ${sy} L ${tx} ${ty}`;

  const distance = Math.max(36, Math.abs(tx - sx) * 0.42);
  if (type === "default") {
    return `M ${sx} ${sy} C ${sx + distance} ${sy}, ${tx - distance} ${ty}, ${tx} ${ty}`;
  }

  if (Math.abs(ty - sy) < 1) return `M ${sx} ${sy} H ${tx}`;
  const midX = (sx + tx) / 2;
  const radius = Math.min(10, Math.abs(ty - sy) / 2, Math.abs(tx - sx) / 4);
  const direction = ty > sy ? 1 : -1;
  return [
    `M ${sx} ${sy}`,
    `H ${midX - radius}`,
    `Q ${midX} ${sy} ${midX} ${sy + direction * radius}`,
    `V ${ty - direction * radius}`,
    `Q ${midX} ${ty} ${midX + radius} ${ty}`,
    `H ${tx}`,
  ].join(" ");
}

export function buildWorkflowSvg({
  title,
  summary,
  nodes,
  edges,
  dark,
}: BuildWorkflowSvgOptions): string {
  const minX = Math.min(...nodes.map((node) => node.x));
  const minY = Math.min(...nodes.map((node) => node.y));
  const maxX = Math.max(...nodes.map((node) => node.x + node.width));
  const maxY = Math.max(...nodes.map((node) => node.y + node.height));
  const width = Math.max(560, Math.ceil(maxX - minX + PADDING_X * 2));
  const height = Math.max(
    220,
    Math.ceil(maxY - minY + HEADER_HEIGHT + PADDING_BOTTOM),
  );
  const offsetX = PADDING_X - minX;
  const offsetY = HEADER_HEIGHT - minY;
  const byId = new Map(nodes.map((node) => [node.id, node]));

  const colors = dark
    ? {
        background: "#17121f",
        dots: "#6b6179",
        card: "#241c30",
        border: "#3a3048",
        title: "#f3eefb",
        subtitle: "#aaa1b8",
        edge: "#8f879c",
      }
    : {
        background: "#f7f9fc",
        dots: "#b9c3d1",
        card: "#ffffff",
        border: "#dbe3ee",
        title: "#253044",
        subtitle: "#8793a5",
        edge: "#8e9aaa",
      };

  const edgeMarkup = edges
    .map((edge) => {
      const source = byId.get(edge.source);
      const target = byId.get(edge.target);
      if (!source || !target) return "";
      const sx = source.x + source.width + offsetX;
      const sy = source.y + source.height / 2 + offsetY;
      const tx = target.x + offsetX;
      const ty = target.y + target.height / 2 + offsetY;
      const path = edgePath(sx, sy, tx, ty, edge.type);
      const label = edge.label
        ? `<text x="${(sx + tx) / 2}" y="${(sy + ty) / 2 - 7}" text-anchor="middle" class="edge-label">${escapeXml(edge.label)}</text>`
        : "";
      return `<g><path d="${path}" class="edge"/>${label}</g>`;
    })
    .join("");

  const nodeMarkup = nodes
    .map((node) => {
      const x = node.x + offsetX;
      const y = node.y + offsetY;
      const color = safeColor(node.color);
      const opacity = node.disabled ? 0.52 : 1;
      return `<g transform="translate(${x} ${y})" opacity="${opacity}">
  <rect width="${node.width}" height="${node.height}" rx="12" fill="${colors.card}" stroke="${colors.border}" filter="url(#shadow)"/>
  <rect x="0" y="10" width="3" height="${Math.max(16, node.height - 20)}" rx="1.5" fill="${color}"/>
  <rect x="14" y="${Math.max(10, (node.height - 30) / 2)}" width="30" height="30" rx="8" fill="${color}" fill-opacity="0.13"/>
  <circle cx="29" cy="${node.height / 2}" r="5" fill="none" stroke="${color}" stroke-width="2"/>
  <circle cx="0" cy="${node.height / 2}" r="4" fill="${color}" stroke="${colors.card}" stroke-width="2"/>
  <circle cx="${node.width}" cy="${node.height / 2}" r="4" fill="${color}" stroke="${colors.card}" stroke-width="2"/>
  <text x="56" y="${node.height / 2 - 3}" class="node-title">${escapeXml(truncate(node.label, 22))}</text>
  <text x="56" y="${node.height / 2 + 15}" class="node-subtitle">${escapeXml(truncate(node.subtitle, 28))}</text>
</g>`;
    })
    .join("");

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeXml(title)}">
  <defs>
    <pattern id="dots" width="24" height="24" patternUnits="userSpaceOnUse">
      <circle cx="1" cy="1" r="1" fill="${colors.dots}" fill-opacity="0.34"/>
    </pattern>
    <filter id="shadow" x="-20%" y="-30%" width="140%" height="170%">
      <feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#0f172a" flood-opacity="${dark ? 0.28 : 0.1}"/>
    </filter>
    <style>
      text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
      .workflow-title { fill: ${colors.title}; font-size: 16px; font-weight: 650; }
      .workflow-meta { fill: ${colors.subtitle}; font-size: 11px; }
      .node-title { fill: ${colors.title}; font-size: 13px; font-weight: 650; }
      .node-subtitle, .edge-label { fill: ${colors.subtitle}; font-size: 10px; }
      .edge { fill: none; stroke: ${colors.edge}; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
    </style>
  </defs>
  <rect width="100%" height="100%" rx="18" fill="${colors.background}"/>
  <rect width="100%" height="100%" rx="18" fill="url(#dots)"/>
  <text x="${PADDING_X}" y="30" class="workflow-title">${escapeXml(truncate(title || "Workflow", 48))}</text>
  <text x="${width - PADDING_X}" y="30" text-anchor="end" class="workflow-meta">${escapeXml(summary)}</text>
  ${edgeMarkup}
  ${nodeMarkup}
</svg>`;
}
