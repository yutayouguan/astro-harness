function decodeJsString(token) {
  if (!token || token === "null") return undefined;
  return JSON.parse(token.replace(/\\x([0-9a-f]{2})/gi, "\\u00$1"));
}

function findAssignedArrays(html, key) {
  const source = html.replaceAll("\0", "");
  const marker = new RegExp(`${key}:\\$R\\[\\d+\\]=\\[`, "g");
  const arrays = [];
  let match;
  while ((match = marker.exec(source))) {
    const start = match.index + match[0].length - 1;
    let depth = 0;
    let quoted = false;
    let escaped = false;
    for (let index = start; index < source.length; index += 1) {
      const char = source[index];
      if (quoted) {
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === '"') quoted = false;
        continue;
      }
      if (char === '"') quoted = true;
      else if (char === "[") depth += 1;
      else if (char === "]") {
        depth -= 1;
        if (depth === 0) {
          arrays.push(source.slice(start + 1, index));
          marker.lastIndex = index + 1;
          break;
        }
      }
    }
  }
  return arrays;
}

function findObjectEnd(source, start) {
  let depth = 0;
  let quoted = false;
  let escaped = false;
  for (let index = start; index < source.length; index += 1) {
    const char = source[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quoted = false;
      continue;
    }
    if (char === '"') quoted = true;
    else if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return -1;
}

function collectServerObjects(source) {
  const records = [];
  const startPattern = /\{id:(?:\d+|"(?:\\.|[^"\\])*")(?:,slug:"|,name:")/g;
  let match;
  while ((match = startPattern.exec(source))) {
    const end = findObjectEnd(source, match.index);
    if (end < 0) break;
    records.push(source.slice(match.index, end));
    startPattern.lastIndex = end;
  }
  return records;
}

function buildObjectReferences(source) {
  const references = new Map();
  const pattern = /\$R\[(\d+)\]=\{/g;
  let match;
  while ((match = pattern.exec(source))) {
    const start = source.indexOf("{", match.index);
    const end = findObjectEnd(source, start);
    if (end < 0) break;
    references.set(match[1], source.slice(start, end));
  }
  return references;
}

function fieldToken(record, name) {
  const match = record.match(
    new RegExp(`(?:\\{|,)${name}:("(?:\\\\.|[^"\\\\])*"|null|!0|!1|-?\\d+)`),
  );
  return match?.[1];
}

function booleanField(record, name) {
  const token = fieldToken(record, name);
  return token === "!0" ? true : token === "!1" ? false : undefined;
}

function buildStringArrayReferences(html) {
  const references = new Map();
  const pattern = /\$R\[(\d+)\]=\[((?:"(?:\\.|[^"\\])*",?)*)\]/g;
  let match;
  while ((match = pattern.exec(html))) {
    const values = match[2]
      ? [...match[2].matchAll(/"(?:\\.|[^"\\])*"/g)].map((item) =>
          decodeJsString(item[0]),
        )
      : [];
    references.set(match[1], values.filter(Boolean));
  }
  return references;
}

function buildDateReferences(html) {
  const references = new Map();
  const pattern = /\$R\[(\d+)\]=new Date\(("(?:\\.|[^"\\])*")\)/g;
  let match;
  while ((match = pattern.exec(html))) {
    references.set(match[1], decodeJsString(match[2]));
  }
  return references;
}

function referencedValue(record, name, references) {
  const match = record.match(new RegExp(`(?:\\{|,)${name}:\\$R\\[(\\d+)\\]`));
  return match ? references.get(match[1]) : undefined;
}

function parseRecord(record, arrayReferences, dateReferences) {
  const idToken = fieldToken(record, "id");
  const decodedId = idToken?.startsWith('"')
    ? decodeJsString(idToken)
    : Number(idToken);
  const slug =
    decodeJsString(fieldToken(record, "slug")) ??
    (typeof decodedId === "string" ? decodedId : undefined);
  const name = decodeJsString(fieldToken(record, "name"));
  if (
    (typeof decodedId !== "string" && !Number.isFinite(decodedId)) ||
    !slug ||
    !name
  ) {
    return undefined;
  }
  return {
    id: decodedId,
    slug,
    name,
    description: decodeJsString(fieldToken(record, "description")) ?? "",
    url: decodeJsString(fieldToken(record, "url")),
    websiteUrl: decodeJsString(fieldToken(record, "websiteUrl")),
    logoUrl:
      decodeJsString(fieldToken(record, "logoUrl")) ??
      decodeJsString(fieldToken(record, "logo")),
    category: decodeJsString(fieldToken(record, "category")) ?? "other",
    tags: referencedValue(record, "tags", arrayReferences) ?? [],
    official: booleanField(record, "official") ?? false,
    featured: booleanField(record, "featured") ?? false,
    updatedAt: referencedValue(record, "updatedAt", dateReferences),
    repoPushedAt: referencedValue(record, "repoPushedAt", dateReferences),
  };
}

export function parsePagination(html) {
  const source = html.replaceAll("\0", "");
  const match = source.match(
    /pagination:\$R\[\d+\]=\{totalPages:(\d+),currentPage:(\d+),totalItems:(\d+),itemsPerPage:(\d+)/,
  );
  return match
    ? {
        totalPages: Number(match[1]),
        currentPage: Number(match[2]),
        totalItems: Number(match[3]),
        itemsPerPage: Number(match[4]),
      }
    : undefined;
}

export function parseMcpServersCollections(html, collectionNames) {
  const source = html.replaceAll("\0", "");
  const arrayReferences = buildStringArrayReferences(source);
  const dateReferences = buildDateReferences(source);
  const objectReferences = buildObjectReferences(source);
  const output = [];

  for (const collection of collectionNames) {
    for (const assignedArray of findAssignedArrays(source, collection)) {
      const records = collectServerObjects(assignedArray);
      for (const reference of assignedArray.matchAll(/\$R\[(\d+)\](?!\s*=)/g)) {
        const record = objectReferences.get(reference[1]);
        if (record) records.push(record);
      }
      const seen = new Set();
      for (const record of records) {
        const server = parseRecord(record, arrayReferences, dateReferences);
        if (server && !seen.has(server.slug)) {
          seen.add(server.slug);
          output.push({ ...server, collection });
        }
      }
    }
  }
  return output;
}
