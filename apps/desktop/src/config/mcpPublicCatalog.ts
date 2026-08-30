import catalog from "./mcp-public-catalog.json";
import communityCatalog from "./mcp-community-catalog.json";
import {
  MCP_PUBLIC_ENTRY_CATEGORY_IDS,
  normalizeMcpServer,
  type McpPublicEntryCategory,
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

type CommunityCatalogEntry = {
  id?: string;
  name?: string;
  description?: string;
  category?: string;
  icon?: string;
  featured?: boolean;
  catalogInstallable?: boolean;
  catalogSource?: string;
  catalogSourceUrl?: string;
  catalogUpstreamUrl?: string;
  catalogNativeCategory?: string;
  catalogOfficial?: boolean;
  catalogSponsored?: boolean;
  catalogTags?: string[];
  catalogUpdatedAt?: string;
  catalogIdentity?: string;
};

function isCatalogCategory(category: string | undefined): category is McpPublicEntryCategory {
  return !!category && MCP_PUBLIC_ENTRY_CATEGORY_IDS.includes(category as McpPublicEntryCategory);
}

/** Validate and normalize the packaged public directory before it reaches the UI. */
function normalizedIdentity(value: string): string {
  return value
    .toLowerCase()
    .replace(/\b(model context protocol|mcp|servers?)\b/g, "")
    .replace(/[^a-z0-9\p{L}]+/gu, "")
    .trim();
}

function loadCuratedCatalog(): McpServer[] {
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
      catalogInstallable: true,
    };
  });
}

function loadCommunityCatalog(curated: McpServer[]): McpServer[] {
  const document = communityCatalog as CatalogDocument;
  if (document.version !== 1 || !Array.isArray(document.servers)) {
    throw new Error("Invalid community MCP catalog document");
  }

  const curatedIdentities = new Set(
    curated.flatMap((entry) => [entry.id, normalizedIdentity(entry.name)]),
  );
  const ids = new Set(curated.map((entry) => entry.id));

  return (document.servers as CommunityCatalogEntry[]).flatMap((entry) => {
    const identity = entry.catalogIdentity ?? normalizedIdentity(entry.name ?? "");
    if (
      !entry.id?.trim() ||
      !entry.name?.trim() ||
      !entry.catalogSourceUrl?.trim() ||
      !isCatalogCategory(entry.category) ||
      entry.catalogInstallable !== false ||
      ids.has(entry.id) ||
      curatedIdentities.has(identity)
    ) {
      return [];
    }
    ids.add(entry.id);
    return [{
      ...normalizeMcpServer({
        ...entry,
        category: entry.category as McpPublicEntryCategory,
        type: "stdio",
        command: "",
        enabled: false,
        websiteUrl: entry.catalogSourceUrl,
      }),
      scope: "builtin" as const,
      provenance: entry.catalogSource ?? "community-catalog",
      editable: false,
      category: entry.category,
      featured: entry.featured === true,
      catalogInstallable: false,
    }];
  });
}

export function loadPublicMcpCatalog(): McpServer[] {
  const curated = loadCuratedCatalog();
  return [...curated, ...loadCommunityCatalog(curated)];
}

export const PUBLIC_MCP_CATALOG = loadPublicMcpCatalog();

/** Convert a read-only directory template into a normal personal MCP config. */
export function installableMcpServer(server: McpServer): McpServer {
  if (server.catalogInstallable === false) {
    throw new Error("Discovery-only MCP entries cannot be installed");
  }
  const runtimeServer = { ...server };
  delete runtimeServer.catalogInstallable;
  delete runtimeServer.catalogSource;
  delete runtimeServer.catalogSourceUrl;
  delete runtimeServer.catalogUpstreamUrl;
  delete runtimeServer.catalogNativeCategory;
  delete runtimeServer.catalogOfficial;
  delete runtimeServer.catalogSponsored;
  delete runtimeServer.catalogTags;
  delete runtimeServer.catalogUpdatedAt;
  delete runtimeServer.catalogIdentity;
  return {
    ...runtimeServer,
    enabled: true,
    scope: "global",
    provenance: "public-catalog",
    editable: true,
    category: undefined,
    featured: undefined,
  };
}
