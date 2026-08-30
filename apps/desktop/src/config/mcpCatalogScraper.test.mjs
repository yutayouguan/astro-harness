import assert from "node:assert/strict";
import test from "node:test";
import {
  parseMcpServersCollections,
  parsePagination,
} from "../../scripts/lib/mcpservers-html.mjs";
import {
  mergeCatalogEntry,
  toRegistryEntry,
} from "../../scripts/sync-mcp-community-catalog.mjs";

const page = `
  servers:$R[1]=[
    $R[2]={id:42,slug:"vendor/example-mcp",name:"Example MCP",description:"示例 \\x3C服务>",url:"https://github.com/vendor/example-mcp",websiteUrl:null,category:"development",tags:$R[3]=["testing","ai-agent"],official:!0,featured:!1,updatedAt:$R[4]=new Date("2026-08-30T01:02:03.000Z"),repoPushedAt:null}
  ],pagination:$R[5]={totalPages:12,currentPage:1,totalItems:350,itemsPerPage:30,hasNextPage:!0,hasPrevPage:!1}
  sponsorServers:$R[6]=[
    $R[7]={id:8,slug:"sponsor",name:"Sponsor",description:"Sponsored",url:"https://example.com",category:"productivity",tags:$R[8]=[],official:!1}
  ]
`;

const remotePage = `
  servers:$R[10]=[
    $R[11]={id:"ahrefs",name:"Ahrefs",description:"SEO 与 AI 搜索分析",originalDescription:"SEO and AI search analytics",logo:"https://example.com/ahrefs.svg"},
    $R[12]={id:"box",name:"Box",description:"文件、文件夹与搜索",originalDescription:"Files, folders and search",logo:void 0}
  ],
  featuredServers:$R[13]=[$R[11]],
  moreServers:$R[14]=[$R[12]]
`;

test("extracts configured SSR collections without evaluating page scripts", () => {
  const [server] = parseMcpServersCollections(page, ["servers"]);
  assert.deepEqual(server, {
    id: 42,
    slug: "vendor/example-mcp",
    name: "Example MCP",
    description: "示例 <服务>",
    url: "https://github.com/vendor/example-mcp",
    websiteUrl: undefined,
    logoUrl: undefined,
    category: "development",
    tags: ["testing", "ai-agent"],
    official: true,
    featured: false,
    updatedAt: "2026-08-30T01:02:03.000Z",
    repoPushedAt: undefined,
    collection: "servers",
  });
});

test("only returns explicitly requested collections", () => {
  assert.equal(parseMcpServersCollections(page, ["servers"]).length, 1);
  assert.equal(parseMcpServersCollections(page, ["sponsorServers"])[0]?.slug, "sponsor");
  assert.equal(parseMcpServersCollections(page, ["featured"]).length, 0);
});

test("extracts pagination metadata from the SSR payload", () => {
  assert.deepEqual(parsePagination(page), {
    totalPages: 12,
    currentPage: 1,
    totalItems: 350,
    itemsPerPage: 30,
  });
});

test("resolves remote MCP collections that contain object references", () => {
  assert.deepEqual(
    parseMcpServersCollections(remotePage, ["featuredServers", "moreServers"]),
    [
      {
        id: "ahrefs",
        slug: "ahrefs",
        name: "Ahrefs",
        description: "SEO 与 AI 搜索分析",
        url: undefined,
        websiteUrl: undefined,
        logoUrl: "https://example.com/ahrefs.svg",
        category: "other",
        tags: [],
        official: false,
        featured: false,
        updatedAt: undefined,
        repoPushedAt: undefined,
        collection: "featuredServers",
      },
      {
        id: "box",
        slug: "box",
        name: "Box",
        description: "文件、文件夹与搜索",
        url: undefined,
        websiteUrl: undefined,
        logoUrl: undefined,
        category: "other",
        tags: [],
        official: false,
        featured: false,
        updatedAt: undefined,
        repoPushedAt: undefined,
        collection: "moreServers",
      },
    ],
  );
});

test("normalizes official Registry entries without treating registry status as vendor official", () => {
  assert.deepEqual(
    toRegistryEntry(
      {
        server: {
          name: "io.example/search",
          title: "Example Search",
          description: "Search example data",
          version: "1.0.0",
          icons: [{ src: "https://example.com/icon.png", mimeType: "image/png" }],
          repository: { url: "https://github.com/example/search" },
          remotes: [{ type: "streamable-http", url: "https://example.com/mcp" }],
          packages: [{ registryType: "npm", identifier: "@example/search" }],
        },
        _meta: {
          "io.modelcontextprotocol.registry/official": {
            updatedAt: "2026-08-30T01:02:03.000Z",
          },
        },
      },
      {
        id: "registry.modelcontextprotocol.io",
        baseUrl: "https://registry.modelcontextprotocol.io",
        path: "/v0/servers",
      },
    ),
    {
      id: "registry-io-example-search",
      name: "Example Search",
      icon: "https://example.com/icon.png",
      description: "Search example data",
      category: "other",
      featured: false,
      catalogInstallable: false,
      catalogSource: "registry.modelcontextprotocol.io",
      catalogSourceUrl: "https://github.com/example/search",
      catalogUpstreamUrl: "https://example.com/mcp",
      catalogRegistered: true,
      catalogRemote: true,
      catalogRegistryName: "io.example/search",
      catalogPackageTypes: ["npm"],
      catalogUpdatedAt: "2026-08-30T01:02:03.000Z",
      catalogIdentity: "examplesearch",
    },
  );
});

test("rejects non-HTTPS Registry icons", () => {
  const entry = toRegistryEntry(
    {
      server: {
        name: "io.example/insecure",
        description: "Insecure icon example",
        icons: [{ src: "http://example.com/icon.png" }],
      },
    },
    {
      id: "registry.modelcontextprotocol.io",
      baseUrl: "https://registry.modelcontextprotocol.io",
      path: "/v0/servers",
    },
  );

  assert.equal(entry.icon, undefined);
});

test("directory metadata enriches matching Registry records", () => {
  const registry = {
    id: "registry-io-example-search",
    name: "Example Search",
    description: "Registry description",
    category: "other",
    catalogSource: "registry.modelcontextprotocol.io",
    catalogSourceUrl: "https://github.com/example/search",
    catalogRegistered: true,
    catalogRemote: true,
    catalogTags: [],
    catalogIdentity: "examplesearch",
  };
  const directory = {
    id: "mcpservers-example-search",
    name: "Example Search MCP",
    description: "中文描述",
    category: "search",
    featured: true,
    catalogSource: "mcpservers.org",
    catalogSourceUrl: "https://mcpservers.org/zh-CN/servers/example-search",
    catalogOfficial: true,
    catalogTags: ["search"],
    catalogIdentity: "examplesearch",
  };
  const merged = mergeCatalogEntry(registry, directory);
  assert.equal(merged.id, registry.id);
  assert.equal(merged.category, "search");
  assert.equal(merged.description, "中文描述");
  assert.equal(merged.catalogRegistered, true);
  assert.equal(merged.catalogOfficial, true);
  assert.equal(merged.catalogDirectoryUrl, directory.catalogSourceUrl);
});
