import catalog from "./mcp-public-catalog.json";
import {
  MCP_PUBLIC_CATEGORY_IDS,
  normalizeMcpServer,
  type McpPublicCategory,
  type McpServer,
} from "../hooks/providers/useMcpTools";

type CatalogEntry = Partial<McpServer> & {
  id?: string;
  name?: string;
  category?: string;
};

type CatalogDocument = {
  version?: unknown;
  servers?: unknown;
};

function isCatalogCategory(category: string | undefined): category is Exclude<McpPublicCategory, "featured"> {
  return !!category && category !== "featured" && MCP_PUBLIC_CATEGORY_IDS.includes(category as McpPublicCategory);
}

/** Validate and normalize the packaged public directory before it reaches the UI. */
export function loadPublicMcpCatalog(): McpServer[] {
  const document = catalog as CatalogDocument;
  if (document.version !== 1 || !Array.isArray(document.servers)) {
    throw new Error("Invalid public MCP catalog document");
  }

  const ids = new Set<string>();
  return (document.servers as CatalogEntry[]).map((entry) => {
    if (!entry.id?.trim() || !entry.name?.trim() || !isCatalogCategory(entry.category)) {
      throw new Error("Invalid public MCP catalog entry");
    }
    if (ids.has(entry.id)) {
      throw new Error(`Duplicate public MCP catalog id: ${entry.id}`);
    }
    ids.add(entry.id);
    const server = normalizeMcpServer(entry);
    return {
      ...server,
      enabled: false,
      scope: "builtin",
      provenance: "public-catalog",
      editable: false,
      category: entry.category,
      featured: entry.featured === true,
    };
  });
}

export const PUBLIC_MCP_CATALOG = loadPublicMcpCatalog();

/** Convert a read-only directory template into a normal personal MCP config. */
export function installableMcpServer(server: McpServer): McpServer {
  return {
    ...server,
    enabled: true,
    scope: "global",
    provenance: "public-catalog",
    editable: true,
    category: undefined,
    featured: undefined,
  };
}
