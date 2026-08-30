import assert from "node:assert/strict";
import test from "node:test";
import {
  parseMcpServersCollections,
  parsePagination,
} from "../../scripts/lib/mcpservers-html.mjs";

const page = `
  servers:$R[1]=[
    $R[2]={id:42,slug:"vendor/example-mcp",name:"Example MCP",description:"示例 \\x3C服务>",url:"https://github.com/vendor/example-mcp",websiteUrl:null,category:"development",tags:$R[3]=["testing","ai-agent"],official:!0,featured:!1,updatedAt:$R[4]=new Date("2026-08-30T01:02:03.000Z"),repoPushedAt:null}
  ],pagination:$R[5]={totalPages:12,currentPage:1,totalItems:350,itemsPerPage:30,hasNextPage:!0,hasPrevPage:!1}
  sponsorServers:$R[6]=[
    $R[7]={id:8,slug:"sponsor",name:"Sponsor",description:"Sponsored",url:"https://example.com",category:"productivity",tags:$R[8]=[],official:!1}
  ]
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
