import { readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseMcpServersCollections,
  parsePagination,
} from "./lib/mcpservers-html.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceConfigPath = resolve(
  appRoot,
  "src/config/mcp-catalog-sources.json",
);

function wait(milliseconds) {
  return new Promise((resolveWait) => setTimeout(resolveWait, milliseconds));
}

async function fetchHtml(url, source) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), source.timeoutMs);
    try {
      const response = await fetch(url, {
        headers: {
          Accept: "text/html",
          "User-Agent": source.userAgent,
        },
        signal: controller.signal,
      });
      if (!response.ok)
        throw new Error(`${response.status} ${response.statusText}`);
      return {
        html: await response.text(),
        lastModified: response.headers.get("last-modified") ?? undefined,
      };
    } catch (error) {
      if (attempt === 2) throw error;
      await wait(500 * 2 ** attempt);
    } finally {
      clearTimeout(timeout);
    }
  }
  throw new Error(`Failed to fetch ${url}`);
}

async function fetchJson(url, source) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), source.timeoutMs);
    try {
      const response = await fetch(url, {
        headers: {
          Accept: "application/json",
          "User-Agent": source.userAgent ?? "Astro-Agent-MCP-Catalog/1.0",
        },
        signal: controller.signal,
      });
      if (!response.ok)
        throw new Error(`${response.status} ${response.statusText}`);
      return {
        document: await response.json(),
        lastModified: response.headers.get("last-modified") ?? undefined,
      };
    } catch (error) {
      if (attempt === 2) throw error;
      await wait(500 * 2 ** attempt);
    } finally {
      clearTimeout(timeout);
    }
  }
  throw new Error(`Failed to fetch ${url}`);
}

function renderPath(template, values) {
  return template.replace(/\{([a-zA-Z]+)\}/g, (_, key) =>
    String(values[key] ?? "")
      .split("/")
      .map((segment) => encodeURIComponent(segment))
      .join("/"),
  );
}

function expandFeeds(source) {
  return source.feeds.flatMap((feed) => {
    if (!feed.expand)
      return [
        { ...feed, path: feed.path ?? renderPath(feed.pathTemplate, source) },
      ];
    const values = source[feed.expand.valuesFrom];
    if (!Array.isArray(values)) {
      throw new Error(`Invalid feed expansion: ${feed.expand.valuesFrom}`);
    }
    return values.map((value) => ({
      ...feed,
      path: renderPath(feed.pathTemplate, {
        ...source,
        [feed.expand.parameter]: value,
      }),
      expandedValue: value,
    }));
  });
}

function pageUrl(source, feed, page) {
  const url = new URL(feed.path, source.baseUrl);
  if (
    url.origin !== new URL(source.baseUrl).origin ||
    url.pathname.startsWith("/api/")
  ) {
    throw new Error(`Unsafe MCP catalog feed: ${url.href}`);
  }
  if (page > 1) url.searchParams.set("page", String(page));
  return url;
}

function mergeServer(target, incoming) {
  const previous = target.get(incoming.slug);
  if (!previous) {
    target.set(incoming.slug, incoming);
    return;
  }
  target.set(incoming.slug, {
    ...previous,
    ...incoming,
    description: previous.description || incoming.description,
    category:
      previous.category !== "other" ? previous.category : incoming.category,
    url: previous.url || incoming.url,
    websiteUrl: previous.websiteUrl || incoming.websiteUrl,
    logoUrl: previous.logoUrl || incoming.logoUrl,
    catalogPath: previous.catalogPath || incoming.catalogPath,
    remoteCatalogPath: previous.remoteCatalogPath || incoming.remoteCatalogPath,
    tags: [...new Set([...previous.tags, ...incoming.tags])],
    official: previous.official || incoming.official,
    featured: previous.featured || incoming.featured,
    sponsored: previous.sponsored || incoming.sponsored,
    remote: previous.remote || incoming.remote,
  });
}

function normalizedName(name) {
  return name
    .toLowerCase()
    .replace(/\b(model context protocol|mcp|servers?)\b/g, "")
    .replace(/[^a-z0-9\p{L}]+/gu, "")
    .trim();
}

function normalizedUrl(value) {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return `${url.hostname.toLowerCase()}${url.pathname}`
      .replace(/\.git$/i, "")
      .replace(/\/$/, "");
  } catch {
    return undefined;
  }
}

function httpsUrl(value, baseUrl) {
  if (!value) return undefined;
  try {
    const url = new URL(value, baseUrl);
    return url.protocol === "https:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function githubAvatarUrl(value) {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    if (url.hostname.toLowerCase() !== "github.com") return undefined;
    const owner = url.pathname.split("/").filter(Boolean)[0];
    return owner
      ? `https://github.com/${encodeURIComponent(owner)}.png?size=64`
      : undefined;
  } catch {
    return undefined;
  }
}

function catalogMatchKeys(server) {
  const keys = [];
  if (server.catalogIdentity) keys.push(`name:${server.catalogIdentity}`);
  for (const value of [server.catalogUpstreamUrl, server.catalogSourceUrl]) {
    const normalized = normalizedUrl(value);
    if (normalized && !normalized.startsWith("mcpservers.org/")) {
      keys.push(`url:${normalized}`);
    }
  }
  return [...new Set(keys)];
}

function toCatalogEntry(server, source, config) {
  const category = config.categoryMap[server.category] ?? "other";
  const icon =
    config.iconAliases[server.slug] ??
    httpsUrl(server.logoUrl, source.baseUrl) ??
    githubAvatarUrl(server.url) ??
    githubAvatarUrl(server.websiteUrl);
  return {
    id: `mcpservers-${server.slug.replace(/[^a-z0-9-]+/gi, "-").toLowerCase()}`,
    name: server.name,
    ...(icon ? { icon } : {}),
    description: server.description,
    category,
    featured: server.featured,
    catalogInstallable: false,
    catalogSource: source.id,
    catalogSourceUrl: new URL(
      server.catalogPath ??
        server.remoteCatalogPath ??
        `/${source.locale}/servers/${server.slug}`,
      source.baseUrl,
    ).href,
    ...(server.remoteCatalogPath
      ? {
          catalogRemoteUrl: new URL(server.remoteCatalogPath, source.baseUrl)
            .href,
        }
      : {}),
    ...(server.url ? { catalogUpstreamUrl: server.url } : {}),
    catalogNativeCategory: server.category,
    catalogOfficial: server.official,
    catalogSponsored: server.sponsored,
    catalogRemote: server.remote === true,
    catalogTags: server.tags,
    ...(server.updatedAt || server.repoPushedAt
      ? { catalogUpdatedAt: server.updatedAt ?? server.repoPushedAt }
      : {}),
    catalogIdentity: normalizedName(server.name),
  };
}

export function toRegistryEntry(item, source) {
  const server = item?.server;
  if (!server?.name || !server?.description) return undefined;
  const title = server.title || server.name.split("/").at(-1) || server.name;
  const remotes = Array.isArray(server.remotes) ? server.remotes : [];
  const packages = Array.isArray(server.packages) ? server.packages : [];
  const icon =
    (Array.isArray(server.icons) ? server.icons : [])
      .map((candidate) => httpsUrl(candidate?.src, source.baseUrl))
      .find(Boolean) ??
    githubAvatarUrl(server.repository?.url) ??
    githubAvatarUrl(server.websiteUrl);
  const remoteUrl = remotes.find(
    (remote) => remote?.type === "streamable-http",
  )?.url;
  const sourceUrl =
    server.websiteUrl ||
    server.repository?.url ||
    new URL(
      `${source.path}?search=${encodeURIComponent(server.name)}`,
      source.baseUrl,
    ).href;
  return {
    id: `registry-${server.name.replace(/[^a-z0-9-]+/gi, "-").toLowerCase()}`,
    name: title,
    ...(icon ? { icon } : {}),
    description: server.description,
    category: "other",
    featured: false,
    catalogInstallable: false,
    catalogSource: source.id,
    catalogSourceUrl: sourceUrl,
    ...(remoteUrl ? { catalogUpstreamUrl: remoteUrl } : {}),
    catalogRegistered: true,
    catalogRemote: remotes.length > 0,
    catalogRegistryName: server.name,
    catalogPackageTypes: [
      ...new Set(packages.map((pkg) => pkg?.registryType).filter(Boolean)),
    ],
    catalogUpdatedAt:
      item?._meta?.["io.modelcontextprotocol.registry/official"]?.updatedAt,
    catalogIdentity: normalizedName(title),
  };
}

async function syncRegistrySource(source) {
  const servers = [];
  let cursor;
  let lastModified;
  for (let page = 1; page <= source.maxPages; page += 1) {
    const url = new URL(source.path, source.baseUrl);
    url.searchParams.set("limit", String(source.limit));
    for (const [key, value] of Object.entries(source.query ?? {})) {
      url.searchParams.set(key, String(value));
    }
    if (cursor) url.searchParams.set("cursor", cursor);
    const response = await fetchJson(url, source);
    lastModified = response.lastModified ?? lastModified;
    for (const item of response.document.servers ?? []) {
      const entry = toRegistryEntry(item, source);
      if (entry) servers.push(entry);
    }
    cursor = response.document.metadata?.nextCursor;
    if (!cursor) break;
    await wait(source.requestDelayMs);
  }
  return { lastModified, servers };
}

async function syncHtmlSource(source, config) {
  const merged = new Map();
  let lastModified;

  const feeds = expandFeeds(source);
  for (let feedIndex = 0; feedIndex < feeds.length; feedIndex += 1) {
    const feed = feeds[feedIndex];
    const first = await fetchHtml(pageUrl(source, feed, 1), source);
    lastModified = first.lastModified ?? lastModified;
    const pagination = parsePagination(first.html);
    const pageCount = Math.min(feed.maxPages, pagination?.totalPages ?? 1);

    for (let page = 1; page <= pageCount; page += 1) {
      const html =
        page === 1
          ? first.html
          : (await fetchHtml(pageUrl(source, feed, page), source)).html;
      const records = parseMcpServersCollections(html, feed.collections);
      for (const record of records) {
        mergeServer(merged, {
          ...record,
          official: record.official || feed.forceOfficial === true,
          featured:
            record.featured ||
            (feed.featuredCollections ?? []).includes(record.collection),
          sponsored: record.collection === "sponsorServers",
          remote: feed.remote === true,
          catalogPath: feed.remote
            ? undefined
            : renderPath(
                feed.detailPathTemplate ?? "/{locale}/servers/{slug}",
                {
                  ...source,
                  slug: record.slug,
                },
              ),
          remoteCatalogPath: feed.remote
            ? renderPath(feed.detailPathTemplate, {
                ...source,
                slug: record.slug,
              })
            : undefined,
        });
      }
      if (page < pageCount) await wait(source.requestDelayMs);
    }
    if (feedIndex < feeds.length - 1) await wait(source.requestDelayMs);
  }

  return {
    lastModified,
    servers: [...merged.values()].map((server) =>
      toCatalogEntry(server, source, config),
    ),
  };
}

export function mergeCatalogEntry(previous, incoming) {
  const registry = previous.catalogRegistered
    ? previous
    : incoming.catalogRegistered
      ? incoming
      : undefined;
  const directory =
    previous.catalogSource === "mcpservers.org"
      ? previous
      : incoming.catalogSource === "mcpservers.org"
        ? incoming
        : undefined;
  const sourceNames = new Set(
    [previous.catalogSource, incoming.catalogSource]
      .flatMap((sourceName) => sourceName?.split(" + ") ?? [])
      .filter(Boolean),
  );
  const previousDirectoryUrl =
    previous.catalogDirectoryUrl ??
    (previous.catalogSource?.includes("mcpservers.org") &&
    !previous.catalogRemoteUrl
      ? previous.catalogSourceUrl
      : undefined);
  const incomingDirectoryUrl =
    incoming.catalogDirectoryUrl ??
    (incoming.catalogSource === "mcpservers.org" && !incoming.catalogRemoteUrl
      ? incoming.catalogSourceUrl
      : undefined);
  const directoryUrl =
    previousDirectoryUrl ??
    incomingDirectoryUrl ??
    previous.catalogRemoteUrl ??
    incoming.catalogRemoteUrl;
  return {
    ...previous,
    ...incoming,
    id: registry?.id ?? previous.id,
    name: directory?.name || registry?.name || incoming.name,
    description:
      directory?.description || registry?.description || incoming.description,
    category:
      directory?.category && directory.category !== "other"
        ? directory.category
        : (registry?.category ?? incoming.category),
    icon: directory?.icon ?? registry?.icon ?? previous.icon ?? incoming.icon,
    featured: previous.featured || incoming.featured,
    catalogInstallable: false,
    catalogSource: [...sourceNames].join(" + "),
    catalogSourceUrl: directoryUrl ?? registry?.catalogSourceUrl,
    catalogDirectoryUrl: directoryUrl,
    catalogUpstreamUrl:
      registry?.catalogUpstreamUrl ?? directory?.catalogUpstreamUrl,
    catalogNativeCategory:
      previous.catalogNativeCategory &&
      previous.catalogNativeCategory !== "other"
        ? previous.catalogNativeCategory
        : incoming.catalogNativeCategory,
    catalogOfficial: previous.catalogOfficial || incoming.catalogOfficial,
    catalogSponsored: previous.catalogSponsored || incoming.catalogSponsored,
    catalogRegistered: registry?.catalogRegistered ?? false,
    catalogRemote: previous.catalogRemote || incoming.catalogRemote,
    catalogRemoteUrl: previous.catalogRemoteUrl ?? incoming.catalogRemoteUrl,
    catalogRegistryName: registry?.catalogRegistryName,
    catalogPackageTypes: registry?.catalogPackageTypes ?? [],
    catalogTags: [
      ...new Set([
        ...(previous.catalogTags ?? []),
        ...(incoming.catalogTags ?? []),
      ]),
    ],
    catalogUpdatedAt: registry?.catalogUpdatedAt ?? directory?.catalogUpdatedAt,
    catalogIdentity: registry?.catalogIdentity ?? directory?.catalogIdentity,
  };
}

async function main() {
  const config = JSON.parse(await readFile(sourceConfigPath, "utf8"));
  if (config.version !== 1 || !Array.isArray(config.sources)) {
    throw new Error("Invalid MCP catalog source configuration");
  }

  const allServers = new Map();
  const lookup = new Map();
  const sourceMetadata = [];
  for (const source of config.sources.filter(
    (candidate) => candidate.enabled,
  )) {
    const result =
      source.kind === "registry-api"
        ? await syncRegistrySource(source)
        : await syncHtmlSource(source, config);
    sourceMetadata.push({
      id: source.id,
      kind: source.kind,
      baseUrl: source.baseUrl,
      lastModified: result.lastModified,
    });
    for (const server of result.servers) {
      const keys = catalogMatchKeys(server);
      const existingId = keys.map((key) => lookup.get(key)).find(Boolean);
      if (existingId && allServers.has(existingId)) {
        const merged = mergeCatalogEntry(allServers.get(existingId), server);
        allServers.set(existingId, merged);
        for (const key of catalogMatchKeys(merged)) lookup.set(key, existingId);
        for (const key of keys) lookup.set(key, existingId);
      } else {
        allServers.set(server.id, server);
        for (const key of keys) lookup.set(key, server.id);
      }
    }
  }

  const servers = [...allServers.values()].sort(
    (left, right) =>
      Number(right.featured) - Number(left.featured) ||
      Number(right.catalogOfficial) - Number(left.catalogOfficial) ||
      left.name.localeCompare(right.name, "zh-CN"),
  );
  const document = {
    version: 1,
    generatedAt: new Date().toISOString(),
    sources: sourceMetadata,
    servers,
  };
  const outputPath = resolve(appRoot, config.output);
  const serialized = `${JSON.stringify(document, null, 2)}\n`;

  if (process.argv.includes("--check")) {
    const current = JSON.parse(await readFile(outputPath, "utf8"));
    const stableSnapshot = (snapshot) => ({
      version: snapshot.version,
      sources: snapshot.sources.map(({ id, kind, baseUrl }) => ({
        id,
        kind,
        baseUrl,
      })),
      servers: snapshot.servers,
    });
    if (
      JSON.stringify(stableSnapshot(current)) !==
      JSON.stringify(stableSnapshot(document))
    ) {
      throw new Error("MCP community catalog snapshot is out of date");
    }
    console.log(`MCP community catalog is current (${servers.length} entries)`);
    return;
  }

  const temporaryPath = `${outputPath}.tmp`;
  await writeFile(temporaryPath, serialized, "utf8");
  await rename(temporaryPath, outputPath);
  console.log(
    `Updated ${config.output} with ${servers.length} discovery entries`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
