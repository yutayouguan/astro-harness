import { readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseMcpServersCollections,
  parsePagination,
} from "./lib/mcpservers-html.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceConfigPath = resolve(appRoot, "src/config/mcp-catalog-sources.json");

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
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
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

function pageUrl(source, feed, page) {
  const url = new URL(feed.path, source.baseUrl);
  if (url.origin !== new URL(source.baseUrl).origin || url.pathname.startsWith("/api/")) {
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
    tags: [...new Set([...previous.tags, ...incoming.tags])],
    official: previous.official || incoming.official,
    featured: previous.featured || incoming.featured,
    sponsored: previous.sponsored || incoming.sponsored,
  });
}

function normalizedName(name) {
  return name
    .toLowerCase()
    .replace(/\b(model context protocol|mcp|servers?)\b/g, "")
    .replace(/[^a-z0-9\p{L}]+/gu, "")
    .trim();
}

function toCatalogEntry(server, source, config) {
  const category = config.categoryMap[server.category] ?? "other";
  const icon = config.iconAliases[server.slug];
  return {
    id: `mcpservers-${server.slug.replace(/[^a-z0-9-]+/gi, "-").toLowerCase()}`,
    name: server.name,
    ...(icon ? { icon } : {}),
    description: server.description,
    category,
    featured: server.featured,
    catalogInstallable: false,
    catalogSource: source.id,
    catalogSourceUrl: new URL(`/${source.locale}/servers/${server.slug}`, source.baseUrl).href,
    ...(server.url ? { catalogUpstreamUrl: server.url } : {}),
    catalogNativeCategory: server.category,
    catalogOfficial: server.official,
    catalogSponsored: server.sponsored,
    catalogTags: server.tags,
    ...(server.updatedAt || server.repoPushedAt
      ? { catalogUpdatedAt: server.updatedAt ?? server.repoPushedAt }
      : {}),
    catalogIdentity: normalizedName(server.name),
  };
}

async function syncSource(source, config) {
  const merged = new Map();
  let lastModified;

  for (const feed of source.feeds) {
    const first = await fetchHtml(pageUrl(source, feed, 1), source);
    lastModified = first.lastModified ?? lastModified;
    const pagination = parsePagination(first.html);
    const pageCount = Math.min(feed.maxPages, pagination?.totalPages ?? 1);

    for (let page = 1; page <= pageCount; page += 1) {
      const html = page === 1 ? first.html : (await fetchHtml(pageUrl(source, feed, page), source)).html;
      const records = parseMcpServersCollections(html, feed.collections);
      for (const record of records) {
        mergeServer(merged, {
          ...record,
          official: record.official || feed.forceOfficial === true,
          featured:
            record.featured || (feed.featuredCollections ?? []).includes(record.collection),
          sponsored: record.collection === "sponsorServers",
        });
      }
      if (page < pageCount) await wait(source.requestDelayMs);
    }
  }

  return {
    lastModified,
    servers: [...merged.values()].map((server) => toCatalogEntry(server, source, config)),
  };
}

async function main() {
  const config = JSON.parse(await readFile(sourceConfigPath, "utf8"));
  if (config.version !== 1 || !Array.isArray(config.sources)) {
    throw new Error("Invalid MCP catalog source configuration");
  }

  const allServers = new Map();
  const sourceMetadata = [];
  for (const source of config.sources.filter((candidate) => candidate.enabled)) {
    const result = await syncSource(source, config);
    sourceMetadata.push({
      id: source.id,
      baseUrl: source.baseUrl,
      lastModified: result.lastModified,
    });
    for (const server of result.servers) allServers.set(server.id, server);
  }

  const servers = [...allServers.values()].sort((left, right) =>
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
      sources: snapshot.sources.map(({ id, baseUrl }) => ({ id, baseUrl })),
      servers: snapshot.servers,
    });
    if (JSON.stringify(stableSnapshot(current)) !== JSON.stringify(stableSnapshot(document))) {
      throw new Error("MCP community catalog snapshot is out of date");
    }
    console.log(`MCP community catalog is current (${servers.length} entries)`);
    return;
  }

  const temporaryPath = `${outputPath}.tmp`;
  await writeFile(temporaryPath, serialized, "utf8");
  await rename(temporaryPath, outputPath);
  console.log(`Updated ${config.output} with ${servers.length} discovery entries`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
